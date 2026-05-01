# `bounce-pretty` — branded HTML bounce wrapper

The `bounce-pretty` binary is a Postfix content-filter that wraps
machine-generated bounce messages (multipart/report DSNs from the
local Postfix bouncer) in branded HTML produced by
`mail_templates::prebuilt::bounce`.

The original `text/plain` Postfix bounce is replaced with a polished
plain alternative; an HTML alternative is added; the
`message/delivery-status` and `message/rfc822` parts are preserved
verbatim so RFC 3464 readers / log scrapers still work.

## What changes for end users

- Bounces show up in mail clients with the brand chrome (gradient
  hero, accent-stripe content card, pill-shaped status eyebrow,
  branded footer) instead of bare plain text.
- Failed-recipient + diagnostic + original-subject are surfaced in
  a labeled "Failed delivery" group card.
- A "Email postmaster" CTA button gives a one-click escalation
  path.

## Build + install

```sh
# On the dev host
cd /home/admin/Mailroom
cargo build --release --bin bounce-pretty

# On the mail host (web-01 today, Hetzner once migrated)
sudo install -o root -g root -m 0755 \
    target/release/bounce-pretty /usr/local/bin/bounce-pretty
```

## Postfix wiring

> **Status as of 2026-05-01**: the pipe transport, header_checks file, and
> `cleanup_no_filter` master.cf entries are staged on web-01 but the live
> `header_checks` parameter is unset because we ran into a recursion
> loop — the standard `cleanup_no_filter` bypass pattern with
> `sendmail -O cleanup_service_name=cleanup_no_filter` did not actually
> route the re-injection through the no-filter cleanup, so the rewritten
> message hit `header_checks` again and triggered the FILTER a second
> time. Postfix's `hopcount_limit` saved us, but it's not a workable
> production state.
>
> Follow-up paths to investigate:
> - Wrap re-injection via `postdrop` instead of `sendmail` so cleanup
>   uses a different submission path.
> - Pipe transport `flags=` overrides — `flags=DRho` may behave
>   differently re: cleanup routing.
> - Switch from FILTER to a milter-based approach (`bounce-pretty` would
>   become a daemon listening on a Unix socket and rewrite via the
>   milter EOM callback). The non_smtpd_milters config slot already
>   exists for OpenDKIM.
> - Replace Postfix's `bounce` master.cf entry entirely with a custom
>   bounce daemon that emits the polished HTML directly.

### 1. Add a pipe transport in `master.cf`

```
bounce_pretty unix - n n - - pipe
  user=nobody argv=/usr/local/bin/bounce-pretty-pipe ${sender} ${recipient}
```

The pipe transport hands the message body to the binary on stdin.
The binary writes the rewritten message to stdout, and Postfix's
sendmail wrapper re-injects.

Wrapper script `/usr/local/bin/bounce-pretty-pipe` (because Postfix's
pipe transport doesn't pipe to sendmail by itself):

```sh
#!/bin/sh
# $1 = sender, $2 = recipient
/usr/local/bin/bounce-pretty | /usr/sbin/sendmail -G -i -f "$1" "$2"
```

Set executable: `sudo chmod 0755 /usr/local/bin/bounce-pretty-pipe`.

### 2. Header check that triggers the filter

`/etc/postfix/non_smtpd_header_checks`:

```
/^From:\s*[^@]+@MAILER-DAEMON/    FILTER bounce_pretty:dummy
/^From:\s*MAILER-DAEMON@/          FILTER bounce_pretty:dummy
/^From:\s*PlausiDen Postmaster/    FILTER bounce_pretty:dummy
```

Match patterns cover Postfix's various bounce From: shapes.

### 3. Wire the header check into `main.cf`

```
postconf -e 'non_smtpd_header_checks = pcre:/etc/postfix/non_smtpd_header_checks'
postfix reload
```

(Install `postfix-pcre` package if not present.)

## Testing

Trigger a real bounce and confirm the filter runs:

```sh
echo test | mail -s 'wrap test' -r william@plausiden.com nobody-$(date +%s)@plausiden.com
sudo journalctl -u postfix -f | grep bounce_pretty
```

You should see a single `bounce_pretty` invocation per bounce, then
the bounce arrives in your inbox with both plain and HTML
alternatives (Thunderbird picks HTML by default).

## Failure mode

- If the binary fails to parse the input or panics, it writes the
  input through unchanged and exits 0. The bounce still gets
  delivered — better degraded than blocked.
- If Postfix's pipe transport itself fails, the bounce is deferred
  and retried per `maximal_queue_lifetime`.

## Edge cases handled

- **Non-bounce messages routed through the filter** — the binary
  detects the absence of `multipart/report` and passes through
  unchanged.
- **Missing diagnostic or status** — falls back to "(no SMTP
  diagnostic)" and the `Other` BounceReason variant.
- **Multi-recipient bounces** — uses the first `Final-Recipient` it
  finds. Other recipients still show in the preserved
  `message/delivery-status` part.
- **Headers with structured values** (DateTime, Address) — copied
  through via `headers_raw()` rather than `HeaderValue::Text`-only
  matching, so Date / From / Return-Path survive.

## Deferred to v0.1

- Per-tenant theme overrides (currently hardcoded to PlausiDen blue).
- A version that replaces the entire Postfix bouncer with a Rust
  daemon — would let us format the original Postfix bounce body
  too, not just the alternative.
- Localized templates.
