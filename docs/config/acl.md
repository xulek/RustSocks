# acl.toml Reference

The ACL file controls which destinations a user or group can reach. Besides these static rules it can hold dynamic `[[policies]]` (schedules, quotas, source networks); see the [Dynamic Policy Engine](policy-engine.md).

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

[[groups]]
name = "developers"

[[groups.rules]]
action = "block"
description = "No private networks"
destinations = ["10.0.0.0/8", "192.168.0.0/16"]
ports = ["*"]
priority = 500
```

Complete example: `docs/examples/acl.example.toml`.

## [global]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `default_policy` | string | `block` | Applied when no rule or policy matches. Values: `allow`, `block`. |

## [[users]]

| Key | Type | Description |
| --- | --- | --- |
| `username` | string | Username to match (exact match). |
| `groups` | array | Groups the user always belongs to. Each must be defined in a `[[groups]]` entry, otherwise the file is rejected. |

## [[users.rules]]

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `action` | string | required | `allow` or `block`. |
| `description` | string | empty | Human readable description. It is reported as the matched rule in logs and the API. |
| `destinations` | array | `[]` | Destination matchers. An empty list matches nothing. |
| `ports` | array | `[]` | Port matchers. An empty list matches nothing. |
| `protocols` | array | `["both"]` | `tcp`, `udp`, or `both`. |
| `priority` | integer | `100` | Higher is evaluated first. |

## [[groups]]

| Key | Type | Description |
| --- | --- | --- |
| `name` | string | Group name. Matching against the user's groups is case-insensitive. |

## [[groups.rules]]

Same fields as `[[users.rules]]`.

A user belongs to the groups listed in their `[[users]]` entry **and** to the groups reported by the system (PAM, NSS/SSSD, LDAP, Active Directory). Reported groups that are not defined in `[[groups]]` are ignored, so you only define the groups you write rules for.

## Matchers

### Destinations

Supported formats:

- Exact IP: `192.168.1.10` (IPv4 or IPv6)
- CIDR: `10.0.0.0/24`, `2001:db8::/32`
- Domain: `example.com`. Case-insensitive and exact: it does not match `www.example.com`
- Wildcard: `*.dev.company.com`. Each `*` stands for **exactly one** DNS label, so `*.example.com` matches `a.example.com` but not `a.b.example.com` or `example.com`, and `api.*.com` matches `api.foo.com`
- Any: `*` on its own matches every destination

A request whose destination is an IP address written as text is matched against the IP and CIDR rules.

### Ports

Supported formats:

- Single: `"22"`
- Range: `"8000-9000"`
- Multiple: `"80,443,8443"`
- Any: `"*"`

### Protocols

`tcp`, `udp`, or `both`.

## Evaluation Order

1. The applicable rules are collected: the user's own rules plus the rules of every group the user belongs to.
2. They are evaluated by `priority`, **highest first**. When two rules have the same priority, `block` is evaluated before `allow`. Priority always comes first: a higher-priority `allow` wins over a lower-priority `block`.
3. The first rule that matches decides, and evaluation stops.
4. If no rule matches, `global.default_policy` is used.

Dynamic policies take part in the same ordering (a policy is evaluated before a legacy rule of equal priority and action).

Example: with the rules below, `10.1.2.3` on port 22 is allowed even though a `block` rule also covers it, because the `allow` has the higher priority. Every other address in `10.0.0.0/8` stays blocked.

```toml
[[users.rules]]
action = "block"
destinations = ["10.0.0.0/8"]
ports = ["*"]
priority = 100

[[users.rules]]
action = "allow"
description = "Jump host is allowed"
destinations = ["10.1.2.3"]
ports = ["22"]
priority = 200
```

### After DNS resolution

A rule that allows a hostname cannot be used to reach an address that an explicit `block` rule covers: after the destination is resolved, every resulting address is checked against the explicit `block` rules again. If no rule matches the resolved address, the decision made for the hostname stands.

## Validation

The file is rejected (and, on a hot reload, the previous configuration stays active) if it contains a duplicate user, group or policy id, a user that references an undefined group, an invalid matcher, or an invalid policy. A rule with no matchers produces a warning.

## Hot Reload

When `[acl].watch = true`, RustSocks reloads the ACL file on change and swaps the compiled configuration atomically. A reload can also be triggered with `POST /api/admin/reload-acl`, and the dashboard and the `/api/acl/*` endpoints edit the file safely (they write a temporary file and rename it). See [ACL Engine](../technical/acl-engine.md).
