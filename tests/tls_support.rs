use rcgen::{
    generate_simple_self_signed, BasicConstraints, Certificate as RcgenCertificate,
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyUsagePurpose,
};
use rustls::{client::ServerName, Certificate, ClientConfig, PrivateKey, RootCertStore};
use rustsocks::acl::AclStats;
use rustsocks::auth::AuthManager;
use rustsocks::config::{AuthConfig, TlsSettings};
use rustsocks::qos::{ConnectionLimits, QosEngine};
use rustsocks::server::{
    create_tls_acceptor, handle_client, ClientHandlerContext, ConnectionPool, PoolConfig,
    TrafficUpdateConfig,
};
use rustsocks::session::SessionManager;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;

fn bind_nonblocking(addr: &str) -> TcpListener {
    let std_listener = StdTcpListener::bind(addr).unwrap();
    std_listener.set_nonblocking(true).unwrap();
    TcpListener::from_std(std_listener).unwrap()
}

#[tokio::test]
async fn socks5_connect_over_tls() {
    let cert = generate_simple_self_signed(["localhost".into()]).unwrap();
    let cert_pem = cert.serialize_pem().unwrap();
    let key_pem = cert.serialize_private_key_pem();

    let temp_dir = tempfile::tempdir().unwrap();
    let cert_path = temp_dir.path().join("server.crt");
    let key_path = temp_dir.path().join("server.key");
    std::fs::write(&cert_path, cert_pem).unwrap();
    std::fs::write(&key_path, key_pem).unwrap();

    let tls_settings = TlsSettings {
        enabled: true,
        certificate_path: Some(cert_path.to_string_lossy().into_owned()),
        private_key_path: Some(key_path.to_string_lossy().into_owned()),
        min_protocol_version: Some("TLS13".to_string()),
        ..Default::default()
    };

    let acceptor = Arc::new(create_tls_acceptor(&tls_settings).unwrap());

    let auth_config = AuthConfig {
        client_method: "none".to_string(),
        socks_method: "none".to_string(),
        users: vec![],
        pam: Default::default(),
        gssapi: Default::default(),
    };
    let auth_manager = Arc::new(AuthManager::new(&auth_config).unwrap());
    let acl_stats = Arc::new(AclStats::new());
    let anonymous_user = Arc::new("anonymous".to_string());
    let session_manager = Arc::new(SessionManager::new());

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: None,
        acl_stats: acl_stats.clone(),
        anonymous_user: anonymous_user.clone(),
        session_manager: session_manager.clone(),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    let socks_listener = bind_nonblocking("127.0.0.1:0");
    let socks_addr = socks_listener.local_addr().unwrap();

    let ctx_clone = ctx.clone();
    let acceptor_clone = acceptor.clone();
    tokio::spawn(async move {
        let (stream, client_addr) = socks_listener.accept().await.unwrap();
        let tls_stream = acceptor_clone.accept(stream).await.unwrap();
        handle_client(tls_stream, ctx_clone, client_addr).await.ok();
    });

    let upstream_listener = bind_nonblocking("127.0.0.1:0");
    let upstream_addr = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut inbound, _) = upstream_listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        inbound.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        inbound.write_all(b"pong").await.unwrap();
    });

    let tcp = TcpStream::connect(socks_addr).await.unwrap();

    let mut root_store = RootCertStore::empty();
    let cert_der = cert.serialize_der().unwrap();
    root_store.add(&Certificate(cert_der)).unwrap();

    let client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(client_config));
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut client = connector.connect(server_name, tcp).await.unwrap();

    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut choice = [0u8; 2];
    client.read_exact(&mut choice).await.unwrap();
    assert_eq!(choice, [0x05, 0x00]);

    let port = upstream_addr.port();
    let connect_request = [
        0x05,
        0x01,
        0x00,
        0x01,
        127,
        0,
        0,
        1,
        (port >> 8) as u8,
        (port & 0xff) as u8,
    ];
    client.write_all(&connect_request).await.unwrap();

    let mut response = [0u8; 10];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response[0], 0x05);
    assert_eq!(response[1], 0x00);

    client.write_all(b"ping").await.unwrap();
    let mut reply = [0u8; 4];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"pong");

    drop(client);
    upstream_task.await.unwrap();
}

#[tokio::test]
async fn socks5_connect_with_mutual_tls() {
    // Generate CA certificate
    let mut ca_params = CertificateParams::new(vec![]);
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params
        .distinguished_name
        .push(DnType::CommonName, "RustSocks Test CA");
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = RcgenCertificate::from_params(ca_params).unwrap();
    let ca_pem = ca_cert.serialize_pem().unwrap();
    let ca_der = ca_cert.serialize_der().unwrap();

    // Generate server certificate signed by CA
    let mut server_params = CertificateParams::new(vec!["localhost".into()]);
    server_params
        .distinguished_name
        .push(DnType::CommonName, "RustSocks Server");
    server_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_cert = RcgenCertificate::from_params(server_params).unwrap();
    let server_pem = server_cert.serialize_pem_with_signer(&ca_cert).unwrap();
    let server_key_pem = server_cert.serialize_private_key_pem();
    // Generate client certificate signed by CA
    let mut client_params = CertificateParams::new(vec![]);
    client_params
        .distinguished_name
        .push(DnType::CommonName, "RustSocks Client");
    client_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client_cert = RcgenCertificate::from_params(client_params).unwrap();
    let client_der = client_cert.serialize_der_with_signer(&ca_cert).unwrap();
    let client_key_der = client_cert.serialize_private_key_der();

    let temp_dir = tempfile::tempdir().unwrap();
    let server_cert_path = temp_dir.path().join("server.crt");
    let server_key_path = temp_dir.path().join("server.key");
    let client_ca_path = temp_dir.path().join("clients-ca.crt");
    std::fs::write(&server_cert_path, server_pem).unwrap();
    std::fs::write(&server_key_path, server_key_pem).unwrap();
    std::fs::write(&client_ca_path, ca_pem).unwrap();

    let tls_settings = TlsSettings {
        enabled: true,
        certificate_path: Some(server_cert_path.to_string_lossy().into_owned()),
        private_key_path: Some(server_key_path.to_string_lossy().into_owned()),
        client_ca_path: Some(client_ca_path.to_string_lossy().into_owned()),
        require_client_auth: true,
        min_protocol_version: Some("TLS13".to_string()),
        ..Default::default()
    };

    let acceptor = Arc::new(create_tls_acceptor(&tls_settings).unwrap());

    let auth_manager = Arc::new(
        AuthManager::new(&AuthConfig {
            client_method: "none".to_string(),
            socks_method: "none".to_string(),
            users: vec![],
            pam: Default::default(),
            gssapi: Default::default(),
        })
        .unwrap(),
    );

    let ctx = Arc::new(ClientHandlerContext {
        auth_manager: auth_manager.clone(),
        acl_engine: None,
        acl_stats: Arc::new(AclStats::new()),
        anonymous_user: Arc::new("anonymous".to_string()),
        session_manager: Arc::new(SessionManager::new()),
        traffic_config: TrafficUpdateConfig::default(),
        qos_engine: QosEngine::None,
        connection_limits: ConnectionLimits::default(),
        connection_pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
    });

    let socks_listener = bind_nonblocking("127.0.0.1:0");
    let socks_addr = socks_listener.local_addr().unwrap();

    let ctx_clone = ctx.clone();
    let acceptor_clone = acceptor.clone();
    tokio::spawn(async move {
        let (stream, client_addr) = socks_listener.accept().await.unwrap();
        let tls_stream = acceptor_clone.accept(stream).await.unwrap();
        handle_client(tls_stream, ctx_clone, client_addr).await.ok();
    });

    let upstream_listener = bind_nonblocking("127.0.0.1:0");
    let upstream_addr = upstream_listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        let (mut inbound, _) = upstream_listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        inbound.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
        inbound.write_all(b"pong").await.unwrap();
    });

    let tcp = TcpStream::connect(socks_addr).await.unwrap();

    let mut root_store = RootCertStore::empty();
    root_store.add(&Certificate(ca_der)).unwrap();

    let client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_store)
        .with_client_auth_cert(vec![Certificate(client_der)], PrivateKey(client_key_der))
        .unwrap();
    let connector = TlsConnector::from(Arc::new(client_config));
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut client = connector.connect(server_name, tcp).await.unwrap();

    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut choice = [0u8; 2];
    client.read_exact(&mut choice).await.unwrap();
    assert_eq!(choice, [0x05, 0x00]);

    let port = upstream_addr.port();
    let connect_request = [
        0x05,
        0x01,
        0x00,
        0x01,
        127,
        0,
        0,
        1,
        (port >> 8) as u8,
        (port & 0xff) as u8,
    ];
    client.write_all(&connect_request).await.unwrap();

    let mut response = [0u8; 10];
    client.read_exact(&mut response).await.unwrap();
    assert_eq!(response[0], 0x05);
    assert_eq!(response[1], 0x00);

    client.write_all(b"ping").await.unwrap();
    let mut reply = [0u8; 4];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"pong");

    drop(client);
    upstream_task.await.unwrap();
}
