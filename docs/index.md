# RustSocks

<div class="rs-hero">
  <div class="rs-tag">SOCKS5 PROXY PLATFORM</div>
  <h1>RustSocks Documentation</h1>
  <p>High-performance SOCKS5 proxy with ACLs, session tracking, QoS, metrics, and a modern web dashboard.</p>
</div>

## What RustSocks Is

RustSocks is a production-ready SOCKS5 proxy server written in Rust. It focuses on performance, observability, and operator control, including:

- Granular ACLs with hot reload
- Real-time session tracking and history
- QoS rate limiting with fair sharing
- Prometheus metrics and diagnostics APIs
- Web dashboard with live monitoring and configuration

<div class="rs-grid">
  <div class="rs-card">
    <h3>Secure by Design</h3>
    <p>Multiple auth methods (none, user/pass, PAM, GSSAPI) with optional dashboard auth.</p>
  </div>
  <div class="rs-card">
    <h3>Operational Visibility</h3>
    <p>Session history, metrics snapshots, telemetry feed, and system health checks.</p>
  </div>
  <div class="rs-card">
    <h3>Control and Automation</h3>
    <p>Runtime settings editor in the UI and API endpoints for management.</p>
  </div>
  <div class="rs-card">
    <h3>Flexible Deployment</h3>
    <p>Build from source or run via Docker with ready-to-use configuration examples.</p>
  </div>
</div>

## Quick Links

- [Quick Start](usage/quick-start.md)
- [Dashboard Overview](ui/overview.md)
- [Configuration Reference](config/rustsocks.md)
- [ACL Rules](config/acl.md)
- [Usage Examples](usage/examples.md)

## Documentation Map

- **Getting Started**: build, run, and first-time setup
- **UI & Dashboard**: pages, workflows, and UI settings
- **Configuration**: full TOML reference for `rustsocks.toml` and `acl.toml`
- **Guides**: advanced setups (dashboard auth, base path, LDAP/AD)
- **Technical**: internal architecture and design notes

If you are new, start with the Quick Start and the dashboard overview.
