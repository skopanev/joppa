# Four levels of the product tree

One workspace represents one product, even when several repositories deliver it.
Use the question each level answers to decide where an item belongs.

```text
Domain                  Which business area is this, and who decides?
  Capability            What can the product do?
    Requirement (REQ)   What behavior do we commit to?
      AC                What concrete condition makes that commitment acceptable?
        Task            What work is needed to meet or verify this condition?
```

Tasks are work attached to ACs, not a fifth level of business classification.
Tests implement checks of ACs. A task being complete does not prove an AC.

## Domain — where a decision belongs

A domain is a business area with a clear decision authority. It is neither a
team nor a system. When a threshold is disputed, the domain identifies who can
decide it. The area survives a team reorganization.

For example, **Growth** describes a business area; **Marketing department**
describes an organizational unit. **Access and subscriptions** identifies the
area where someone decides what a subscription includes.

A domain has no independent delivery status. “Compliance is done” does not say
which commitments have been checked. Its displayed progress is a summary below it.

## Capability — what we can do

Complete “We can …”. If the result describes a product ability, it is a capability.

| Candidate | Interpretation |
|---|---|
| We can screen transactions | Capability |
| We can withdraw funds | Capability |
| We can restore paid access | Capability |
| Build an operator screen | Task or implementation choice |
| admin-api | Repository or system component |

A capability groups commitments about an ability. It has no independent truth
or delivery verdict; its progress is derived from its requirements.

## Requirement — a product commitment

A requirement states behavior you can ask “Is this true now?” about.

| Wording | Requirement? |
|---|---|
| Incoming transfers above 5,000 go to manual review | Yes |
| Canceling a subscription preserves access until the paid period ends | Yes |
| Improve screening | No: the result is unspecified |
| Build an operator screen | No: that describes work |

Describe how the product behaves. Separate unrelated behaviors joined by “and”
into separate commitments when they can be accepted independently.

### The REQ card

Each REQ has one owner who answers for its meaning and accepts its result.
Each REQ names one or more consumers from the workspace consumer list. A consumer
is a role or group that receives the same promised behavior, not necessarily a
workspace member. Every AC applies to all consumers of its REQ; ACs have no
separate consumer field. If behavior or acceptance differs between consumers,
write separate REQs. Correcting the consumer list on an existing REQ creates a
new revision under the same ID, with all AC checks reset as usual.
People and agents formulate REQ and ACs together. Agents help make conditions
clear and testable; authorized people decide business policy and scope.

The card contains wording, ACs, discussion and revision history. Anyone finding
a gap leaves a comment there. The owner accepts, clarifies or rejects the
suggestion with a reason, obtaining a specialist decision where needed. There
is no separate proposal entity or approval workflow to maintain.

Each saved change to the REQ or its ACs creates a new REQ revision. History
preserves who changed what and when, including previous wording and removed
ACs. Each revision owns its check results and acceptance. **Every AC in a new
revision starts at n/a**, including unchanged ones. Comments alone do not create
revisions. See the [process](process.md).
Changing the capability or consumer list uses the same REQ ID and a new revision
with a reason. Previous revisions keep their original capability and consumers.
Sources can be linked to a revision with a type, URI, source version and relation
to the requirement. Linking or withdrawing a source records an event; previous
revision sources remain available in history.

### Immutable history — WORM

Saved revisions, comments, check results and decisions are append-only. Correct
a comment or withdraw a decision by adding a linked record. Removing an AC from
a new revision does not erase it from earlier revisions.

Revision state is computed from immutable records. PostgreSQL rejects UPDATE,
DELETE and TRUNCATE of the journal. This is application/database WORM, not a
promise against a database superuser; see [storage and trust](storage.md).

## AC — one acceptance condition

An AC states a concrete condition under which a requirement is acceptable.
It describes **what must hold**; a test describes **how to check it**.

```text
REQ  Amounts at or above the reporting threshold are reportable.
  AC  The threshold is 10,000.00 inclusive.
      Test  9,999.99 is not reportable.
      Test  10,000.00 is reportable.
      Test  10,000.01 is reportable.
  AC  Comparison uses the unrounded amount.
      Test  9,999.996 is not rounded up before comparison.
  AC  The operation records the exchange rate, its source and capture time.
      Test  All three fields are present after crediting.
```

A REQ describes the whole commitment; an AC describes one condition for accepting
it. Both are checkable statements. Their difference is scope, not whether a
statement can be true. One boundary condition can need three tests: below,
exactly at and above the boundary. That is one AC, not three.

### AC scope

Work stays inside the agreed AC and context. If a client matters, name it in the
AC: “On iOS, paid access survives signing in again.” That does not add Android or
Web. Adding platforms or other behavior requires a human scope decision.

When the client is material but unspecified, ask before implementation. Missing
scope never means “all supported platforms.” Agents may choose additional tests
of the agreed condition, such as active and expired subscriptions, without
inventing new product commitments.

## Decision procedure

| Question | Place |
|---|---|
| Which business area owns this decision? | Domain |
| What can the product do? | Capability |
| What behavior do we promise as a whole? | REQ |
| What individual condition makes that promise acceptable? | AC |
| What work implements or verifies the condition? | Task attached to ACs |
| How is the condition checked? | Test or manual check |

## Size signals, not validation rules

A few domains (often 4–8), a few capabilities per domain (often 3–8), and a small
set of ACs (often 2–6) usually remain readable. A very large set may hide multiple
commitments; a single AC may merely repeat an overly narrow REQ. These are review
prompts, not quotas. Use as many conditions and tests as the actual commitment
requires. Never manufacture ACs to reach a target count.

## Creating and maintaining the tree

Agents can propose domains and capabilities. The workspace owner confirms the
tree. An agent can clarify an unconfirmed title, but cannot change the domain's
authority or edit a confirmed item. Confirmation is an explicit human decision.

Creation through MCP can omit local IDs; the server generates them and returns
both resolved command fields and permanent public IDs. Keep IDs when revising
existing objects. Archived objects retain their identity and history; create a
new object for genuinely new scope. See [API authoring](api.md#automatic-ids-and-batches).
