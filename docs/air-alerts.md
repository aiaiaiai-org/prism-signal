# Air-threat relay

The first product built on Prism Signal: relay what a public channel reports about drones, guided bombs, and missiles to the people it concerns, and to nobody else.

The only thing a user gives is a location. Relevance does the rest: a report about Kyiv reaches people near Kyiv, and a report about Kherson does not. The first source is one channel, [`t.me/vanek_nikolaev`](https://t.me/vanek_nikolaev), read through its public preview.

This document maps that product onto the ecosystem boundaries, says what exists, and lists the decisions that are still open. It changes no ownership rule in [`architecture.md`](architecture.md) or [`flows.md`](flows.md).

## Not an official warning

The channel is an unofficial, human-run source. An alert built from it must say so, name the channel, and link the original post. The bot must tell every user, before their first alert, that silence is not safety and that official air-raid alerts remain the authority. Where an official feed is added later it becomes another source with its own reliability class, and its messages outrank this channel's.

## Pipeline

| Stage | Owner | What it does | State |
| --- | --- | --- | --- |
| Collect | `prism-signal-source-telegram`, run by the hub | Reads new posts from the channel preview | built |
| Normalize | `prism-signal-normalize` | Post text to hazard readings with place roles | built, see [`normalization.md`](normalization.md) |
| Cover | `prism-signal-geo` | Place and reach radius to a set of grid cells | designed, [`geo-cells.md`](geo-cells.md) |
| Fuse | `prism-signal-fusion` | Readings in a window to an assessment with a lifecycle | designed, [`architecture.md`](architecture.md) |
| Subscribe | `prism-hub` | Who wants which kinds, near which cell | not started |
| Gate | `prism-hub` | Issuer policy: may this assessment be sent, to whom | not started |
| Render | `prism-porter` | `signal.alert` artifact to a delivery intent, in `uk-UA` | not started |
| Deliver | `prism-hub` delivery worker and `prism-bot` | Push to a Telegram chat | path built for mail digests; not yet for alerts |
| Locate | `prism-bot` | Ask for and receive the user's location | not started |

Prism Signal never sees a user, a chat, or a subscription. The hub owns all three.

## Location

The user shares a location once through Telegram's location button, or shares a live location that Telegram keeps updating. Alternatively they pick a place from a list and give no coordinates at all. The bot sends it to the hub; nothing downstream needs more than a coarse cell.

```text
user location -> hub -> cell (coarse) stored against the subscription
reading place + reach_km -> cover -> cell set
alert if subscription cell is in the cell set and the kind matches
```

Recommendation, for the hub and `0x1` to decide:

- **store a cell, not a coordinate.** The exact point is used once to derive the cell and is not kept;
- **use a coarse resolution.** A reading's reach is 5 to 20 km, so a subscription cell of roughly 3 km edge (H3 resolution 6) resolves it. Resolution 7, the `0x1` broadcast grid, is finer than the data warrants and ties a chat to a neighbourhood;
- **let the user delete it.** `/stop` removes the subscription and the cell. The hub keeps delivery metadata only if the operator's policy says so.

A chat identifier is still needed to reach a bot user, so a subscription is location tied to identity at the hub. It is coarse, but it is not anonymous, and the bot's first message should say so plainly.

## Relevance

A reading is relevant to a subscription when all of these hold:

1. the reading is a `threat` with a known `kind`, and the user subscribed to that kind;
2. one of its places has role `target` or `via`; `origin` and `mention` never alert;
3. the subscription cell lies inside the cover of that place's `reach_km`.

`via` is a weaker signal than `target`. A hub may send `via` only to users who chose an early-warning level, or word it differently. That is a hub policy.

A `cleared` reading ends the threat it names. The hub sends a follow-up to whoever received the alert: a wrong or finished alert is corrected on the path that produced it.

## Freshness

The channel does not always call a threat off. Time from a kind's last threat report to the channel's own all-clear, in the 495-post sample of 26 days (only threats it explicitly called off, so a lower bound on how long silence should be trusted):

| Kind | Called off | Median | 90th percentile | Longest |
| --- | --- | --- | --- | --- |
| drone | 39 | 8 min | 22 min | 28 min |
| guided bomb | 12 | 4 min | 9 min | 13 min |
| cruise missile | 7 | 3 min | 10 min | 10 min |
| missile | 15 | 6 min | 18 min | 67 min |
| ballistic missile | 3 | 3 min | 6 min | 6 min |

This is evidence for the validity window of a `FusionPolicy`, not a value to hard-code: the sample is small and one channel wide. Expiry belongs to fusion; the hub only relays what fusion issues.

## What exists

- a collector that reads the channel and streams `Evidence` (`prism-signal-collect`);
- a normalizer that turns it into readings (`prism-signal-normalize`), measured on 495 posts;
- in `prism-hub` and `prism-bot`: a mail-digest scheduler and delivery worker in the hub, a service-to-service delivery endpoint in the bot, and per-user lifecycle (`/stop`, `/resume`). The hub's own documents list general scheduling as a later increment. Neither has subscriptions or location yet;
- in `prism-porter`: deterministic rendering and idempotency keys for other artifacts, not for `signal.alert`.

## What is missing, by repository

1. **`prism-signal`**: `cover` (H3, bounded, deterministic), a `SignalObservation` contract built from a reading, fusion v1 for one hazard kind and one source, the assessment lifecycle, and the `normalize`, `cover`, `assess` operations of `prism-signal.v1`.
2. **`prism-hub`**: a subscription model (actor, kinds, coarse cell), a scheduler that runs the collector and stores the observation window, the issuer-policy gate for unofficial sources, and an audience query by cell and kind.
3. **`prism-porter`**: a `signal.alert` renderer in `uk-UA` that names the source, links the post, states the age of the report, and carries a stable idempotency key so a retry never sends twice.
4. **`prism-bot` and `prism-hubot`**: request and receive a location (including live-location edits), choose kinds, `/stop`, and the not-official notice.
5. **Other messengers**: additional delivery adapters behind the hub. `prism` already has WhatsApp direct-message and Channel boundaries; the messaging-window and template rules of each provider decide whether unsolicited alerts are possible there. Telegram comes first because the delivery path exists.

## Decisions that are open

| Decision | Owner |
| --- | --- |
| Whether the channel's owner is contacted and agrees to being relayed; the preview is a public courtesy, not an API | product |
| Whether alerts carry the channel's text or only a structured summary and a link (a summary and a link is the safer default) | product |
| Subscription cell resolution, and whether it lives in a `0x1`-bound profile | `0x1`, hub |
| Whether `via` alerts are sent, and to whom | hub policy |
| Regions: how a report about `Киевская область` reaches users, which needs administrative boundaries | this repository |
| Validity windows per kind | fusion policy, with evidence |
| Alert cool-down so a burst of posts about one threat sends one message | hub policy |
| Which second messenger, and whether it can carry unsolicited alerts | product |
