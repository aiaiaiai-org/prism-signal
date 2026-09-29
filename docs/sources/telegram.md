# Telegram public channel preview

`prism-signal-source-telegram` reads the public web preview at `https://t.me/s/<channel>`.

## Why the preview

| Option | Credentials | History | Chosen |
| --- | --- | --- | --- |
| Web preview `t.me/s/…` | none | full, page by page via `?before=` | yes, first adapter |
| Bot API | bot token | only chats where the bot is a member or admin | no: cannot read foreign channels |
| MTProto user API | `api_id`, `api_hash`, a user session | full | later, if the preview proves insufficient |

The preview exists only when the channel owner keeps it enabled. A missing preview is reported as `NotFound` (`telegram.preview.unavailable` or `telegram.preview.disabled`), never as an empty page.

## What becomes evidence

| Preview element | `Evidence` field |
| --- | --- |
| `data-post="<channel>/<id>"` | `external_id = <channel>/<id>` |
| `time[datetime]` | `published_at`, converted to UTC |
| `edited` in the meta line | `edited = true` |
| `.js-message_text` | `text`, `<br>` as newlines; a quoted reply is not the body |
| photo, video, round video, voice, document | `media[]` with kind and CDN URL when present (photo background, video file, else video thumbnail); bytes are never fetched |
| "media not supported" notice | one `media[]` item of kind `other`, only when the post has no other media: Telegram also nests this notice inside every video player and repeats it after supported media |
| forwarded-from name | `forwarded_from` |
| post link | `provenance.url` |

Service messages (channel created, photo changed) are skipped. View counts are not collected: they change on every read and would break deduplication.

CDN media URLs expire. They are recorded as provenance hints, not as durable references.

## Paging

- `Latest` → `/s/<channel>`; the page exposes `older` from the "load more" link and `newest` from its highest id.
- `Before(id)` → `?before=<id>`, `After(id)` → `?after=<id>`.
- The adapter drops posts outside the requested window, so the port's strict window holds even if the server returns edge posts.

## Collecting

```bash
# whole history, newest pages first, one Evidence per line
cargo run -p prism-signal-collect -- telegram vanek_nikolaev backfill > vanek.ndjson

# only new posts, polling every 60 s after a known id
cargo run -p prism-signal-collect -- telegram vanek_nikolaev follow --after 12345 >> vanek.ndjson
```

`(source_id, external_id)` is the deduplication key. An edited post is emitted again with `edited = true` only if it is re-read; the collector does not detect edits on its own.

The collector waits `--delay-ms` (default 1500 ms) between pages and backs off on `429` and transient failures. Keep it polite: the preview is a public courtesy, not an API with a published quota.

## Known limits

- Parsing depends on Telegram's preview markup, which is undocumented and may change. Tests run on two pages captured verbatim from the live channel on 2026-09-29 (`tests/fixtures/live_*.html`) and on a synthetic fixture for cases those pages lack (service messages, standalone unsupported media). Recapture when the markup drifts.
- An album is one post under its first id; the other ids in the album never appear as posts.
- Polls, stickers, and location posts are not yet mapped to dedicated fields; their text, if any, is still collected.
- Edits and deletions after collection are not observed.
