# ACL Engine - Implementation Guide

This document explains how the ACL engine (`src/acl/`) is built: how the configuration is compiled, how a request is evaluated, and how hot reload works. For the configuration format see the [acl.toml reference](../config/acl.md) and the [Dynamic Policy Engine](../config/policy-engine.md).

## Overview

The engine answers one question for every new connection, BIND peer and UDP datagram: *may this user reach this destination?* Its inputs are the user, the user's groups, the source address, the authentication method, the destination, port and protocol, the current time and the user's current usage. Its output is allow or block, plus the rule or policy that decided it.

| File | Responsibility |
| --- | --- |
| `types.rs` | Configuration types: `AclConfig`, `AclRule`, `UserAcl`, `GroupAcl`, `AccessPolicy` |
| `matcher.rs` | Compiled destination, port and protocol matchers |
| `policy.rs` | Policy conditions, admission limits, usage tracker, evaluation context and outcome |
| `engine.rs` | `AclEngine`: compilation, evaluation, reload |
| `loader.rs`, `persistence.rs`, `crud.rs` | Loading, atomic saving and editing the ACL file |
| `watcher.rs` | Hot reload |
| `metrics.rs`, `stats.rs` | Decision metrics and per-user counters |

## From configuration to compiled rules

The TOML configuration stores matchers as strings. When the engine is created or reloaded they are compiled once:

- **Destinations** become one of: match-all (`*`), IP, CIDR (`ipnet`), exact domain (lowercased, compared case-insensitively) or a wildcard compiled to a regular expression. A `*` label stands for exactly one DNS label, so `*.example.com` becomes `^[^.]+\.example\.com$`
- **Ports** become single, range, list or any
- **Rules** (`CompiledAclRule`) hold the action, description, priority, protocols and the compiled matchers. They are shared with `Arc`, so putting a rule in several lists costs nothing
- Each user's rules and each group's rules are **sorted once** at compile time by the evaluation order
- Users are indexed by exact username, groups by lower-cased name (so a group reported by LDAP as `Developers` finds `[[groups]] name = "developers"` with a single hash lookup)
- Enabled policies are compiled (conditions, schedule parsed to minutes and a weekday mask, source networks) and sorted the same way

The whole compiled configuration lives behind `Arc<RwLock<CompiledAclConfig>>`. Evaluation takes a read lock; a reload builds a new compiled configuration first and swaps it in under a short write lock.

## Evaluation

`AclEngine::evaluate_policy_for_traffic` is the per-connection entry point. `evaluate_policy_with_context` returns the same decision plus a trace and is used by the Explain API (`POST /api/acl/test`).

### Ordering

Candidates (legacy rules and policies) are considered in this order:

1. higher `priority` first
2. at equal priority, `block` before `allow`
3. at equal priority and action, a policy before a legacy rule

Priority always dominates: a higher-priority `allow` beats a lower-priority `block`. The first candidate that decides ends the evaluation.

### No sorting per request

Every source is already sorted: the user's rules, the rules of each group the user belongs to, and the policy list. Evaluation therefore merges those sorted lists lazily, always taking the best head, and stops at the first match. Nothing is copied or sorted per request, and a request that matches an early rule does not look at the rest.

```text
sources = [user rules] + [rules of each matching group] + [policies]
loop:
    pick the best head among the sources     (priority, block-before-allow, policy-before-rule)
    legacy rule  -> matches?        yes: return its action
    policy       -> subject matches? target matches? mode? conditions?   (see below)
default policy
```

Group handling: the user's static `groups` and the groups reported by the system are combined and de-duplicated case-insensitively; a group that appears twice contributes its rules once.

### Policies in the merge

For a policy the engine checks, in order: the subject (user or group), the target (destination, port, protocol), then its mode and conditions:

- `mode = "monitor"`: counted in `monitor_matches` if it would have applied, never changes the result
- conditions fail and `enforce_conditions = true` (a gate): the request is blocked
- conditions fail otherwise: the policy is skipped and evaluation continues with lower candidates
- conditions hold: the policy decides, and an allow carries its admission limits (active connections, rate, quotas) back to the caller

### Cost control

- The destination is analysed once per request (`PreparedDestination`: lowercased domain, parsed IP) instead of once per rule
- User names and groups are lower-cased once, and only when a policy needs them
- Condition explanations are built only when a trace is requested
- The traffic path builds **no trace**, which is the main saving: a trace costs several allocations per candidate

### Outcome

`PolicyEvaluationOutcome` carries the decision, the matched rule description, the matched policy id, the admission limits, the `DecisionSource` (legacy rule, policy or default), the number of matching monitor policies and, for the Explain path only, the ordered trace.

### After DNS resolution

For a hostname the decision is made before resolution. Afterwards `is_explicitly_blocked_with_policy_context` evaluates each resolved address again and reports a block only when an explicit rule or policy produced it, so a hostname that was allowed cannot reach an address that an explicit `block` covers, while a request with no matching rule for the resolved address keeps the hostname decision. The same check is applied to BIND peers and UDP destinations.

## Where it is used

| Flow | When |
| --- | --- |
| CONNECT (SOCKS5 and SOCKS4/4a) | Before DNS, then once per resolved address |
| BIND | When the request is received, and again for the connecting peer |
| UDP ASSOCIATE | When the association is opened (admission limits) and for every datagram |

Each decision updates `rustsocks_acl_decisions_total`; an allow reserves its admission limits before the connection is established.

## Hot reload

`AclWatcher` (`watcher.rs`) keeps the engine in step with the file when `acl.watch = true`:

1. Filesystem events from `notify` (with a 1 s polling interval), plus a polling fallback for environments where events are unreliable
2. A content fingerprint avoids reloading the same file twice and avoids retrying a file that already failed
3. The new file is loaded and validated, then compiled
4. The compiled configuration is swapped in atomically
5. On any error the previous configuration stays active and the error is logged
6. Reloads that take longer than 100 ms log a warning

A reload can also be triggered with `POST /api/admin/reload-acl`. Edits made through the dashboard or the `/api/acl/*` endpoints are written atomically (temporary file, validation, rename, with a backup that is restored on failure) and then reloaded.

Established sessions are not affected by a reload; dynamic policies are evaluated when a session is opened.

## Validation

`AclConfig::validate` rejects: duplicate users, groups or policy ids; users that reference an undefined group; invalid matchers (bad CIDR, port or wildcard); and invalid policies (empty targets, bad schedule, limits of zero, a gate on a block policy). A rule without matchers only warns.

## Performance

Measured with `cargo bench --bench policy_evaluation` (Criterion, release build, one developer machine; compare revisions rather than treating the numbers as absolute). The target matches the lowest-priority entry, which is the worst case for a walk:

| Candidates | Per-connection path | Explain path (with trace) |
| --- | --- | --- |
| 10 legacy rules | about 1 µs | about 5 µs |
| 50 legacy rules | about 2 µs | about 17 µs |
| 200 legacy rules | about 7 µs | about 65 µs |
| 200 policies with conditions | about 11 µs | about 90 µs |

Reloading never blocks evaluation for longer than the pointer swap.

## Configuration best practices

1. **Use `default_policy = "block"`** and allow what is needed: a whitelist is easier to audit.
2. **Plan priorities deliberately.** Higher numbers are evaluated first, and the first match decides. Give a specific exception a higher priority than the broad rule it overrides, in either direction (a specific `allow` above a broad `block`, or a specific `block` above a broad `allow`).
3. **Use groups** for common rule sets and let users inherit them.
4. **Use unique priorities** for rules that could overlap, so the outcome never depends on the tie-break.
5. **Try before you enforce.** Create new policies in `monitor` mode, watch `rustsocks_policy_monitor_matches_total`, and use the Explain simulator.
6. **Watch the file:**

```toml
[acl]
enabled = true
config_file = "config/acl.toml"
watch = true
```

## Observability and management

```text
GET    /api/acl/rules                 all rules
GET    /api/acl/global                default policy        (PUT to change)
GET    /api/acl/groups, /api/acl/users     groups and users with their rules (POST/PUT/DELETE to edit)
GET    /api/acl/policies              dynamic policies      (POST/PUT/DELETE to edit)
POST   /api/acl/search                find rules by criteria
POST   /api/acl/test                  Explain: decision plus trace
POST   /api/admin/reload-acl          reload the file
```

Reads (`GET`) need the `viewer` role; every other method needs `admin` (this includes the `search` and `test` requests, which are `POST`s) and is written to the audit log. Decisions are exported as Prometheus metrics; see [Metrics & Audit Log](../guides/metrics-and-audit.md).

## Testing

- Unit tests live next to the code (`engine.rs`, `policy.rs`, `matcher.rs`)
- `tests/acl_unit.rs`, `acl_integration.rs`, `acl_api.rs`, `acl_management_api.rs` and `policy_engine.rs` cover rules, the API and policies
- The previous evaluation algorithm is kept as a test-only reference, and a property test checks on random configurations that the current implementation agrees with it, trace included

```bash
cargo test --all-features acl
cargo test --all-features --test policy_engine
```
