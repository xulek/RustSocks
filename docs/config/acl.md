# acl.toml Reference

The ACL file controls which destinations a user or group can reach.

## Structure

```toml
[global]
default_policy = "block"

[[users]]
username = "alice"
groups = ["developers"]

[[users.rules]]
action = "allow"
description = "Allow HTTP/HTTPS"
destinations = ["*"]
ports = ["80", "443"]
protocols = ["tcp"]
priority = 100
```

Complete example: `docs/examples/acl.example.toml`.

## [global]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `default_policy` | string | `block` | Applied when no rules match. Values: `allow`, `block`. |

## [[users]]

| Key | Type | Description |
| --- | --- | --- |
| `username` | string | Username to match. |
| `groups` | array | Group names associated with the user. |

## [[users.rules]]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `action` | string | required | `allow` or `block`. |
| `description` | string | empty | Human readable description. |
| `destinations` | array | `[]` | Destination matchers. |
| `ports` | array | `[]` | Port matchers. |
| `protocols` | array | `["both"]` | `tcp`, `udp`, or `both`. |
| `priority` | integer | `100` | Higher is evaluated first. |

## [[groups]]

| Key | Type | Description |
| --- | --- | --- |
| `name` | string | Group name. |

## [[groups.rules]]

Same fields as `[[users.rules]]`.

## Matchers

### Destinations

Supported formats:

- Exact IP: `192.168.1.10`
- CIDR: `10.0.0.0/24`
- Domain: `example.com`
- Wildcard: `*.dev.company.com` or `*`

### Ports

Supported formats:

- Single: `"22"`
- Range: `"8000-9000"`
- Multiple: `"80,443,8443"`
- Any: `"*"`

### Protocols

`tcp`, `udp`, or `both`.

## Evaluation Order

1. Rules are sorted so **block rules** are evaluated before **allow rules**.
2. Within the same action, higher `priority` is evaluated first.
3. Rules from user and group scopes are combined, then sorted.
4. If no rule matches, `global.default_policy` is used.

## Hot Reload

When `[acl].watch = true`, RustSocks reloads the ACL file on change and swaps the config atomically.
