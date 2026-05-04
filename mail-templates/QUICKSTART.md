# mail-templates — QUICKSTART

Branded HTML/plain transactional email rendering for the PlausiDen
mail stack. Typed AST, hand-written renderers, no template engine.

## 30-second pitch

You have an `EmailDocument` (a typed tree of blocks). You call
`.render_html()` and `.render_plain()`. You attach the two as
`multipart/alternative` and ship via SMTP. Done.

The chrome (gradient hero, pill eyebrows, accent-stripe content
cards, branded footer) is hard-coded into the renderer; the AST
controls only content + structure. So every email in the stack
looks like every other email — magic-link, bounce, alert, weekly
digest, DNS dispatch — without each call site rebuilding the
chrome.

## Rust API — 5-line minimum

```rust
use mail_templates::{Block, EmailDocument};

let doc = EmailDocument {
    subject: "Hello".into(),
    preheader: "First line in inbox preview.".into(),
    eyebrow: Some("Greeting".into()),
    heading: "Hi from PlausiDen".into(),
    intro: None,
    blocks: vec![Block::Paragraph { text: "A short message body.".into() }],
    footer_lines: vec![],
};
let html = doc.render_html();
let plain = doc.render_plain();
```

`html` is a complete `<!DOCTYPE html>` document with inline styles —
ready as the `text/html` part of a `multipart/alternative`. `plain`
is wrapped at 78 cols with structural rule strings.

## Prebuilt builders

For the recurring shapes the stack already produces, skip authoring
the AST and use a builder. All in `mail_templates::prebuilt`:

| Builder | When |
|---|---|
| `magic_link(link)` | Single-use sign-in link emails |
| `password_reset(link, expires_min)` | Account-flow password reset |
| `email_verification(link, expires_hours)` | Sign-up email confirmation |
| `feedback_received(...)` | Notify team@ when public form posts |
| `inquiry_received(...)` | Notify team@ on /contact form |
| `dns_records(groups, dig_commands)` | DNS dispatch with nested record cards |
| `bounce(to, failed_recipient, reason, diag, subj)` | NDR rewrap |
| `alert(severity, title, summary, fields, runbook, on_call)` | Ops / monitoring |
| `weekly_digest(period, headline, rows, extras)` | Periodic summary |

Each returns an `EmailDocument` you render the same way:

```rust
use mail_templates::prebuilt::{alert, AlertSeverity};
use mail_templates::Field;

let doc = alert(
    AlertSeverity::Critical,
    "Disk usage 95% on web-01",
    "/var crossed the high-water mark.",
    vec![
        Field { label: "Host".into(),       value: "web-01".into(), mono: true  },
        Field { label: "Mountpoint".into(), value: "/var".into(),   mono: true  },
        Field { label: "Used".into(),       value: "95%".into(),    mono: false },
    ],
    Some("https://runbooks.plausiden.com/disk-full"),
    Some("oncall@plausiden.com"),
);
println!("{}", doc.render_html());
```

## Shell — `mail-tpl` (generic JSON → render)

Pipe an `EmailDocument` JSON to `mail-tpl` for HTML, plain, or a
ready-to-send MIME envelope. Useful when JSON is easier than Rust:

```sh
# HTML alternative only
echo '{"subject":"Hi","preheader":"x","heading":"Hello",
       "blocks":[{"kind":"paragraph","text":"body"}],"footer_lines":[]}' \
  | mail-tpl > out.html

# Multipart envelope; pipe straight to sendmail
cat doc.json | mail-tpl --mime | sendmail -t

# SacredVote tenant chrome instead of PlausiDen
cat doc.json | mail-tpl --theme sacredvote
```

## Shell — `mail-alert` (cron-friendly ops alerts)

For monitoring scripts, systemd timers, cron jobs — wraps
`prebuilt::alert` with a tighter arg surface so a one-line shell
pipeline works:

```sh
mail-alert --severity critical \
    --title "Disk 95% on web-01" \
    --summary "/var crossed high-water; mail will stall at 100%." \
    --field @Host=web-01 --field Used=95% \
    --runbook https://runbooks.plausiden.com/disk-full \
    --on-call oncall@plausiden.com \
    --to ops@plausiden.com --mime \
  | sendmail -t
```

The `@`-prefix on a field name (`@Host=web-01`) renders the value in
a monospaced code box. Without `@`, the value is regular prose.

`mail-pulse` (separate crate / shell script in PlausiDen-Email-Config)
uses `mail-alert` to fire transition-only alerts when a probe goes
warn/crit; that's the canonical example of the sender pattern.

## Multi-tenant theming

Brand colors + footer text live in a `Theme`. Two pre-built themes
ship today:

```rust
use mail_templates::Theme;

let html = doc.render_html_with_theme(&Theme::plausiden());   // default
let html = doc.render_html_with_theme(&Theme::sacredvote());  // alt tenant
```

Every color in the chrome (eyebrows, button shadows, the how-to
callout's tint, the gradient hero) derives from `theme.brand_primary`
via `rgba_from_hex` + `darken` — so a tenant only sets their primary
brand color, the rest of the palette tracks automatically.

`Theme` is `Serialize` / `Deserialize` (serde-stable), so future
admin-UI editing of the chrome via a JSON ledger is a drop-in.

## Migration path from bespoke renderers

If you have a hand-rolled `format!()`/`maud` email renderer in your
codebase (plausiden-site/src/views/email.rs is the canonical example),
the migration is:

1. Identify which prebuilt matches your shape (`feedback_received`
   for a feedback notification, `inquiry_received` for a contact-form
   inquiry, etc.).
2. Replace the body of your `fn ..._html(...)` with a call to that
   prebuilt's builder + `.render_html()`.
3. Drop the now-unused chrome helpers (your local `shell()`,
   `escape_html()`, etc).
4. Tests: keep the assertions, drop them on the chrome details
   (those move into mail-templates' tests). Keep tests on the
   user-content escaping + on field presence.

Net effect: one less duplicated HTML chrome implementation, all
emails in the stack inherit chrome upgrades automatically.

## What this crate doesn't do

- **SMTP send.** No lettre, no transport. Pair with `lettre` or pipe
  through `sendmail`. Keeps the rendering deterministic + testable
  without network.
- **Localization.** Single-language for v0. The footer + intro copy
  is English. Localize at the call site by swapping the strings you
  pass in; the AST is language-agnostic.
- **Inline images / attachments.** No CID embedding. Use external
  `<img src="https://...">` only if you must (best practice: don't,
  Gmail strips them in some configurations).
- **Click-tracking pixels.** Deliberately excluded — we don't
  instrument transactional mail.

## File map

```
mail-templates/
├── src/
│   ├── lib.rs           — EmailDocument, Block, GroupCard, ...
│   ├── escape.rs        — HTML escape (5 metacharacters)
│   ├── tokens.rs        — Theme struct, FONT_SANS / FONT_MONO
│   ├── render_html.rs   — typed AST → text/html
│   ├── render_plain.rs  — typed AST → text/plain
│   ├── prebuilt.rs      — magic_link / bounce / alert / ... builders
│   └── bin/
│       ├── mail-tpl.rs    — JSON → render CLI
│       └── mail-alert.rs  — cron-friendly alert CLI
└── QUICKSTART.md         (this file)
```
