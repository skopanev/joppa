# Configurable workspace model

Each workspace defines its own tree schema.
All objects have common document revisions, comments, attachments and links.
Read [Objects and workspace trees](objects.md) for the current contract.
The model below describes the recommended tree and its verification roles.

# The recommended tree

Write new technical text in English according to ASD-STE100, Issue 9. At least 80% of sentences in each text must comply.
Russian is permitted under the same rules. Preserve intake, verbatim user text, existing wording, history, and evidence.
Read the [technical writing rules](writing.md). The REQ owner approves new REQ and AC wording with an acceptance.

One workspace represents one product, even when several repositories deliver it.
Use the question each level answers to decide where an item belongs.

```text
Domain                  Which business area is this, and who decides?
  Capability            What can the product do?
    Requirement (REQ)   What behavior do we commit to?
      AC                What observable condition makes that commitment acceptable?
        Test            Which test proves the condition?
        Task            What work implements the condition? (created on demand)
Tech                    Which shared technical area does this root own?
  Requirement (REQ)     Which technical contract do we commit to?
    AC                  What exact contract makes that commitment acceptable?
      Test, Task        The same as under a product AC.
Inbox (RAW)             What did the source report, and who sent it?
Source (SRC)            Which decision, specification or standard grounds the rules, at which version?
```

Product rules use the vertical Domain → Capability → REQ → AC → Test.
Technical contracts use the vertical Tech → REQ → AC → Test. One vertical applies everywhere:
REQ → AC → Test, and a Task under the AC.
Tasks are work attached to an AC, not a level of business classification.
Tests implement checks of AC nodes. A Task does not prove a criterion.

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

## Tech — shared technical contracts

`tech` is one root type for the technical contracts of database, API, transport, security and
infrastructure. Its root nodes are areas, for example API, Database, Integrations, Platform,
Security and Infra. Each area holds REQ, AC and Test nodes, the same as a capability.
Link work across areas and capabilities with `depends_on`.

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
Each AC names its Sources in its field `sources`. The REQ gets no copy of them.
When the schema defines `ac.consumer`, each AC names exactly one consumer from the
workspace consumer list, in its field `consumer`. A consumer is a role, a group or an application that receives the promised
behavior, not necessarily a workspace member. A green AC therefore closes one consumer
without doubt. A REQ and every level above it name no consumer. If two consumers need
the same behavior, write one AC for each consumer.
People and agents formulate REQ and ACs together. Agents help make conditions
clear and testable; authorized people decide business policy and scope.

The card contains wording, ACs, discussion and revision history. Anyone finding
a gap leaves a comment there. The owner accepts, clarifies or rejects the
suggestion with a reason, obtaining a specialist decision where needed. There
is no separate proposal entity or approval workflow to maintain.

Each saved change to the REQ or its ACs creates a new REQ revision. History
keeps who changed what and when, with the previous wording and removed ACs.
A change of a node resets the criteria at and below it: they are grey until a
newer test record arrives. Comments do not create revisions. See the
[process](process.md) and [what green means](green.md).
A move to another capability keeps the REQ ID and adds a revision with a reason.

### Immutable history — WORM

Saved revisions, comments, check results and decisions are append-only. Correct
a comment or withdraw a decision by adding a linked record. Removing an AC from
a new revision does not erase it from earlier revisions.

Revision state is computed from immutable records. PostgreSQL rejects UPDATE,
DELETE and TRUNCATE of the journal. This is application/database WORM, not a
promise against a database superuser; see [storage and trust](storage.md).
The one exception is the deletion of a workspace by its owner. It removes every
row of that workspace, its journal included.

## AC — observable conditions

An AC states an observable condition under which a requirement is acceptable.
Choose the format that fits:

1. `Given / When / Then` — for a state transition or action.
2. Direct rule — for an invariant, limit or disclosure where `Given / When / Then` is unnatural.

Under a Tech root, an AC states one exact technical contract, for example a route and input
with its HTTP status and payload, or a table, key, TTL or threshold.
Each AC links the Sources that ground it. Tests (`kind: "test"`) and Tasks (`kind: "task"`)
attach to an AC.

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

A REQ describes the whole commitment. An AC describes one observable condition for accepting it.

## Sources and the Inbox

An Inbox item (badge RAW) is a raw incoming signal before analysis. A Source (badge SRC) is an
owner decision, a specification or an external standard with a fixed version. A comment
(`op: "note"`) is a discussion inside a card. Do not mix these three items.
After analysis, the agent puts the lasting content into a Source and into REQ and AC nodes.
Then the agent archives the Inbox item. See [Inbox items and Sources](objects.md#inbox-items-and-sources).

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
| Which shared technical area owns this contract? | Tech root, such as API or Database |
| What behavior do we promise as a whole? | REQ |
| What observable condition makes that promise acceptable? | AC |
| Which decision, specification or standard grounds the AC? | Source, in `sources` of the AC |
| What raw signal arrived, before analysis? | Inbox item |
| What work implements the condition? | Task under the AC, created directly before implementation |
| How is the condition checked? | Test node under the AC |

## Size signals, not validation rules

A few domains (often 4–8), a few capabilities per domain (often 3–8), and a small
set of ACs (often 2–6) usually remain readable. A very large set may hide multiple
commitments; a single AC may merely repeat an overly narrow REQ. These are review
prompts, not quotas. Use as many conditions and tests as the actual commitment
requires. Never manufacture ACs to reach a target count.

## Creating and maintaining the tree

Agents and people save domains, Tech roots, capabilities, REQs and ACs with `save_object`.
Only an owner acceptance approves a REQ revision. Do not create Tasks while the tree is
filled or agreed. Create a Task directly before the implementation of its AC.

Creation through MCP can omit local IDs: the server makes them and returns the
resolved command. Keep IDs when you edit existing objects. Archived objects keep
their identity and history. Create a new object for new scope. See
[generated IDs and batches](api.md#generated-ids-and-batches).
