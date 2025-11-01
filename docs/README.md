# RustSocks Documentation

Complete documentation for RustSocks SOCKS5 proxy server.

## 📚 Documentation Structure

### User Guides (`guides/`)

End-user focused documentation:

- **[LDAP Groups Guide](guides/ldap-groups.md)** - How to configure and use LDAP group integration
- **[Building with Base Path](guides/building-with-base-path.md)** - Complete guide for building and deploying with URL prefix

### Technical Documentation (`technical/`)

Implementation details and architecture:

- **[ACL Engine](technical/acl-engine.md)** - Access Control List engine implementation
- **[PAM Authentication](technical/pam-authentication.md)** - Pluggable Authentication Modules integration
- **[Session Manager](technical/session-manager.md)** - Session tracking and persistence
- **[LDAP Integration](technical/ldap-integration.md)** - LDAP groups technical implementation

### Examples (`examples/`)

Example configuration files:

- **`rustsocks.example.toml`** - Full server configuration example
- **`acl.example.toml`** - ACL rules configuration example

Copy these to `config/` and modify as needed.

## 🚀 Quick Start

See the main [README.md](../README.md) in the project root.

## 📖 Main Documentation

- **[README.md](../README.md)** - Project overview and quick start
- **[CLAUDE.md](../CLAUDE.md)** - Comprehensive guide for development (AI-friendly)

## 🏗️ Project Structure

```
RustSocks/
├── docs/                      # Documentation (you are here)
│   ├── guides/               # User guides
│   ├── technical/            # Technical documentation
│   └── examples/             # Example configurations
├── dashboard/                # Web admin dashboard (React)
├── config/                   # Active configuration files
├── examples/                 # Rust code examples
├── src/                      # Source code
└── tests/                    # Integration tests
```

## 🔗 External Resources

- **API Documentation**: Available at `/swagger-ui/` when API is enabled
- **Dashboard**: Web UI at `http://127.0.0.1:9090/` when enabled

## 📝 Contributing

When adding documentation:

1. **User guides** go in `guides/` - focus on "how to use"
2. **Technical docs** go in `technical/` - focus on "how it works"
3. **Examples** go in `examples/` - working configuration files
4. Keep main `README.md` concise - link to detailed docs here

## 🆘 Getting Help

- Check [CLAUDE.md](../CLAUDE.md) for comprehensive project documentation
- Review relevant guides in `guides/`
- See example configurations in `examples/`
- Check API documentation via Swagger UI
