# Dynamic Access Policy Engine

RustSocks supports dynamic access policies in addition to the legacy ACL model. Policies are evaluated in the same priority space as legacy user/group rules and add context that ordinary destination ACLs cannot express: source network, authentication method, schedules, validity windows, connection admission limits and transfer quotas.

## Evaluation model

For each new connection RustSocks builds a policy context containing the authenticated user, effective groups, source IP, authentication method, requested destination/port/protocol, current UTC time and current per-user usage counters.

Legacy ACL candidates and dynamic policies are ordered by `priority` descending. At the same priority a `block` candidate wins over `allow`, and if the action is also the same, a policy is evaluated before a legacy rule (so a failing `enforce_conditions` gate cannot be bypassed by an equally-ranked legacy `allow`). Disabled policies are ignored.

`mode = "enforce"` participates in the decision. `mode = "monitor"` is evaluated and appears in Explain traces but cannot change the effective result. This allows a policy to be observed before it is enforced.

For an ALLOW policy, `enforce_conditions = true` enables **gate** behavior. Once the policy subject and target match, failure of a dynamic condition blocks the request instead of falling through to a lower-priority allow. This is appropriate for rules such as ‚ÄúDevelopers may access GitHub only during business hours.‚Äù

## Policy shape

```toml
[[policies]]
id = "developers-github"
enabled = true
mode = "enforce"
description = "Developers may use GitHub during business hours"
groups = ["developers"]
action = "allow"
destinations = ["github.com", "*.github.com"]
ports = ["443"]
protocols = ["tcp"]
priority = 1500
enforce_conditions = true
owner = "network-team"
ticket = "CHG-184"
tags = ["production"]

[policies.conditions]
source_ips = ["10.0.0.0/8"]
auth_methods = ["userpass", "gssapi"]
max_active_connections = 20
max_connections_per_minute = 60
daily_transfer_limit_bytes = 10737418240
monthly_transfer_limit_bytes = 107374182400

[policies.conditions.schedule]
days = ["mon", "tue", "wed", "thu", "fri"]
start = "07:00"
end = "20:00"
utc_offset_minutes = 120
```

An empty `users` and `groups` scope is global. Destinations accept IP addresses, CIDR networks, exact domains and wildcard domains. Ports use the same syntax as legacy ACLs, including individual ports, ranges and `*`.

## Dynamic conditions

### Source networks

`source_ips` accepts single IPv4/IPv6 addresses and CIDR ranges. A non-empty list requires the client source IP to match at least one entry.

### Authentication method

`auth_methods` accepts `none`, `userpass`, `pam.address`, `pam.username` and `gssapi`. RustSocks uses the configured authentication backend identity rather than only the SOCKS wire-level method, so PAM and built-in username/password policies can be distinguished.

### Time schedule

The schedule uses local policy time calculated from UTC plus `utc_offset_minutes`. `days = []` means every day. `start`/`end` use `HH:MM`. Overnight windows are supported, for example `22:00` to `06:00`.

### Validity window

`not_before` and `expires_at` are RFC3339 timestamps. `expires_at` is exclusive. These are useful for temporary vendor or incident-response policies.

### Admission limits and quotas

Available admission controls are:

- `max_active_connections`
- `max_connections_per_minute` (rolling 60-second window)
- `daily_transfer_limit_bytes`
- `monthly_transfer_limit_bytes`

The counters are maintained by a dedicated in-memory policy usage tracker and do not depend on bounded session history or the selected SQL backend. Active/rate admission is reserved atomically so concurrent handshakes cannot all observe the same pre-admission count.

Policy Engine v1 treats these limits as **admission limits**. When a quota becomes exhausted, an already established TCP stream is not terminated mid-transfer; new sessions are denied. UDP destinations continue to receive source/auth/time/target policy checks while transfer quotas are enforced when the association is admitted.

## Running several instances

By default each instance keeps its own counters in memory, so with N instances behind a load
balancer a limit of 20 active connections effectively allows about 20 x N, and daily/monthly
quotas reset whenever an instance restarts. To enforce limits for the whole deployment, share
the counters through Redis:

```toml
[policy_state]
backend = "redis"
redis_url = "redis://:${REDIS_PASSWORD}@redis.internal:6379/0"
key_prefix = "rustsocks-prod"
failure_mode = "fail_closed"
```

Build with the `redis` feature (`cargo build --release --features redis`).

How it behaves:

- **Admission is atomic.** The limit checks and the reservation run as one Redis script, so
  concurrent connections to different instances cannot all pass the same pre-admission count.
- **No leaked connections.** Active connections are leases that each instance renews while
  they are open. If an instance crashes, its connections stop counting once `lease_secs` has
  elapsed. Graceful closes release immediately.
- **Quotas lag slightly.** Proxied bytes are batched and flushed about once per second, so
  another instance may see a user's transfer up to roughly a second late. Quotas are admission
  limits, so this only affects how soon a new connection is refused.
- **Redis outages.** With `fail_closed`, new connections under a policy that has limits are
  denied while Redis is unreachable; with `fail_open` they are admitted using this instance's
  local counters (limits then apply per instance until Redis returns). Policies without limits
  are never affected. Watch `rustsocks_policy_usage_store_errors_total`.
- **Startup.** An unreachable Redis is reported at startup in both modes.
- **Clock skew** between instances does not matter: lease and rate arithmetic uses Redis time.
- **Cost.** Each new connection adds two Redis round trips (a usage read and the atomic
  reservation). On a loopback Redis this measured about 0.2 ms each, and a single instance
  sustained roughly 12,000 reservations per second over one connection. Expect your network
  round-trip time on top, and place Redis close to the instances.

Other things to plan for when running several instances:

- UDP ASSOCIATE relays are bound to the instance that accepted the control connection, so the
  load balancer must keep a client's TCP control connection and its UDP traffic on one instance.
- Keep the ACL file identical on every instance (for example from a shared config source);
  policy edits made through the API change only the instance that received them.

## DNS safety

Domain policy evaluation occurs before resolution and resolved IP addresses are checked again before connecting/sending. An explicit CIDR block therefore cannot be bypassed by a hostname resolving into a blocked private or local network.

## Monitor rollout

A recommended rollout is:

1. Create the policy with `mode = "monitor"`.
2. Use the dashboard Explain Simulator and operational traces to verify expected matches.
3. Change to `mode = "enforce"` once the behavior is understood.

Monitor matches are visible in the trace but do not affect the final decision.

## REST API

Policy CRUD:

- `GET /api/acl/policies`
- `POST /api/acl/policies`
- `GET /api/acl/policies/{id}`
- `PUT /api/acl/policies/{id}`
- `DELETE /api/acl/policies/{id}`

Policy IDs are stable and case-insensitively unique. `PUT` cannot silently rename an ID. Mutating ACL endpoints require the administrator role under dashboard RBAC.

## Explain / Simulator

`POST /api/acl/test` accepts normal connection data plus dynamic context:

```json
{
  "user": "alice",
  "groups": ["developers"],
  "source_ip": "10.1.2.3",
  "auth_method": "userpass",
  "destination": "github.com",
  "port": 443,
  "protocol": "tcp",
  "now": "2026-09-08T10:00:00Z",
  "usage": {
    "active_connections": 3,
    "connections_last_minute": 12,
    "bytes_today": 1048576,
    "bytes_this_month": 20971520
  }
}
```

`now` and `usage` are optional. When omitted, current time and live usage are used. The response contains the effective decision, matched policy ID/rule and an ordered trace explaining target/condition matches and why candidates did or did not become effective.

The web dashboard exposes the same functionality under **Access Policies**.

The Explain trace is built only for this endpoint. Per-connection evaluation reaches the same decision without it, which keeps the decision path cheap: with 200 rules it takes roughly 8-11 µs, against about 64-88 µs when a full trace is produced.

## Legacy ACL compatibility

Existing `[global]`, `[[users]]` and `[[groups]]` configurations continue to work. The `policies` array defaults to empty when omitted. Dynamic policies are intentionally not used to terminate already-established TCP sessions during ACL hot reload; admission conditions are evaluated when opening a new session. Legacy ACL hot-reload behavior remains available for legacy rules.

See `config/examples/acl-policies.toml` for a complete example.
