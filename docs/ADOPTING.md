# Adopting Mailroom — for SacredVote and future tenants

How to bring Mailroom up for a new tenant (a domain we operate the mail stack for), keep it in sync with upstream Mailroom over time, and contribute generic improvements back without leaking tenant-specific secrets.

This is the doc the SacredVote operator (and any future tenant) reads first.

## The lineage, restated

```
Mailroom (this repo)                     ← generic, all-Rust mail-stack toolkit
   │
   ├── PlausiDen-Email-Config            ← plausiden.com tenant overlay
   │
   └── SacredVote-Email-Config (you)     ← sacredvote.org tenant overlay
```

Mailroom owns the *generic* code: the `CategoryRule` schema, the Sieve emitter, the postfix/dovecot/opendkim templates, the mail-cli. Each tenant overlay owns *only the tenant-specific bits*: which mailboxes exist, which DKIM keys are published, which env-file values are live, which DNS records have been set at the registrar.

## Setting up SacredVote-Email-Config (one-time)

1. **Create the private repo** on github: `thepictishbeast/SacredVote-Email-Config`. Description:
   `🔒 PRIVATE — SacredVote-tenant overlay for Mailroom. Captures /etc on the mail VPS + DNS state + secret-regen recipes. Never make public.`

2. **Bootstrap it from PlausiDen-Email-Config as a template:**

   ```sh
   git clone https://github.com/thepictishbeast/PlausiDen-Email-Config.git \
     SacredVote-Email-Config
   cd SacredVote-Email-Config
   rm -rf .git
   git init -b main
   git remote add origin https://github.com/thepictishbeast/SacredVote-Email-Config.git
   ```

3. **Edit the captured config** to be SacredVote-specific:
   - `postfix/main.cf` — change `myhostname`, `mydomain`, `myorigin`.
   - `postfix/vmailbox` — replace mailbox list with SacredVote's.
   - `opendkim/KeyTable` + `SigningTable` — replace with sacredvote.org signing config.
   - `nginx/*` — replace plausiden.com vhosts with sacredvote.org ones.
   - `dovecot/users.example` — replace mailbox list (regen hashes per `docs/SECRETS.md`).
   - `env/*.example` — replace env-file templates with SacredVote's.
   - `docs/DNS-RECORDS.md` — fill in sacredvote.org's actual records.
   - `docs/REPOS-INDEX.md` — adjust to SacredVote's repo set.

4. **Update the README** to say SacredVote, not PlausiDen.

5. **First commit:**

   ```sh
   git add -A
   git commit -m "Initial SacredVote-Email-Config — bootstrapped from PlausiDen-Email-Config template"
   git push -u origin main
   ```

## Pulling Mailroom updates

Mailroom is the upstream of generic code (mail-cli, sieve emitter, deployment patterns). Periodically you'll want to pull its updates. **You don't pull Mailroom into your config repo** — you pull Mailroom into your build environment.

```sh
# On the VPS that runs SacredVote's mail stack:
cd ~/Mailroom-build
git pull origin main
cargo build --release -p mail-cli
sudo install target/release/mail-admin /usr/local/bin/

# Then re-emit the global Sieve rules + restart the milter:
sudo /usr/local/bin/mail-admin emit-categories \
    --output /etc/dovecot/sieve/categories.sieve --force
sudo sievec /etc/dovecot/sieve/categories.sieve
sudo systemctl reload dovecot
```

If a Mailroom update changes a deployment template (e.g. a new `postfix/master.cf` recommendation), you decide whether to adopt it in your config repo. Diff Mailroom's template against your `postfix/master.cf` and merge what makes sense.

## Contributing improvements upstream

If you fix something in your tenant config that's actually a generic improvement (e.g. you find a better way to wire MTA-STS), it should land in Mailroom so every tenant benefits.

The rule of thumb:

| Change type | Lands in |
|---|---|
| New typed rule, new section variant, new `mail-admin` subcommand, deployment-template improvement | **Mailroom** |
| Mailbox added, DKIM rotated, DNS record published, env-file value changed | **your tenant overlay** |

Process:

```sh
# Branch in Mailroom for the generic improvement:
cd ~/Mailroom-build
git checkout -b mta-sts-helper
# … edit, test …
git commit -m "mail-cli: emit-mta-sts subcommand"
git push -u origin mta-sts-helper
# Open a PR. Once merged, every tenant runs `git pull` + rebuild.
```

The tenant overlay never depends on uncommitted Mailroom changes.

## Reporting bugs / asking questions

- **Bugs** in Mailroom (the generic code): file an issue on `thepictishbeast/Mailroom`.
- **Bugs** in your tenant config (your specific deployment): file an issue on your own private overlay repo.
- **Cross-tenant questions** (e.g. "is this a deployment pattern issue or a code issue?"): start in Mailroom; if it's tenant-specific, it gets moved.

## Secret hygiene

The same rules as PlausiDen-Email-Config:

- **Never commit private DKIM keys** (`*.private`).
- **Never commit real env-file values.** Templates only, with `<REDACTED-N+chars>` placeholders.
- **Never commit unredacted dovecot password hashes.** Use `users.example`, regenerate hashes via `doveadm pw -s SHA512-CRYPT`.
- **Real values live in `/etc/...` on the VPS, mode 600.** Backed up out-of-band (sealed env file in 1Password / paper / etc.), not in git.

If you accidentally commit a real secret: rotate immediately (regen recipes in `docs/SECRETS.md`), force-push the redacted version, and revoke the leaked credential at its source.
