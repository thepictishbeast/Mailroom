# Thunderbird laptop client — recommended configuration

This is the recipe to make a fresh Thunderbird laptop install behave like
the rest of the Mailroom stack expects: every folder syncs, every
folder pushes notifications, drag-and-drop move works, and an
"All Mail" view collects everything.

The IMAP server is `mail.plausiden.com`. The same recipe works for any
@plausiden.com user, and the playbook is intentionally generic so other
tenants (SacredVote, etc.) can reuse it.

> Run the steps under "Account-level settings" once **per account** if
> you've added more than one mailbox.

---

## 1) Subscribe to every folder, sync them, check them on a schedule

In Thunderbird:

1. **Right-click the account name → Subscribe…** Tick every folder
   listed (Inbox, Sent, Drafts, Trash, Junk, Archive, Important,
   Updates, Social, Promotions, Forums) and OK.
2. **Right-click each folder → Properties → Synchronization**:
   - Tick **"Select this folder for offline use"**
   - Tick **"Check this folder for new messages"**
3. Account Settings → **Server Settings → Advanced**:
   - **Maximum number of server connections to cache**: `5`
     (`5` lets Thunderbird IDLE on five folders simultaneously — the
     usual sweet spot for `Inbox + Updates + Social + Promotions +
     Forums`. Bumping higher than `10` triggers `Too many connections
     from this IP` from dovecot.)
   - Tick **"Use IDLE command if the server supports it"**
4. Account Settings → **Synchronization & Storage**:
   - Tick **"Keep messages for this account on this computer"**
   - Choose **"Synchronize all messages locally regardless of age"**
     (or the age that suits the disk).

## 2) Make every folder push notifications, not just Inbox

By default Thunderbird only shows the toast / sound / tray badge for
the Inbox folder. To get the same alert when a message lands in any
other folder (because most of our real mail lands in Updates or
Important after sieve), set these in Settings → General → Config Editor
(or Edit → Preferences → General → Config Editor):

| Pref                                                              | Value          |
|-------------------------------------------------------------------|----------------|
| `mail.server.default.check_all_folders_for_new`                   | `true`         |
| `mail.server.default.check_time`                                  | `5`            |
| `mail.biff.show_alert`                                            | `true`         |
| `mail.biff.show_tray_icon`                                        | `true`         |
| `mail.biff.play_sound`                                            | `true`         |
| `mail.biff.use_system_alert`                                      | `true`         |
| `mail.notification.show_count`                                    | `true`         |
| `mail.notification.show_subject`                                  | `true`         |
| `mail.notification.show_sender`                                   | `true`         |
| `mailnews.use_correct_reply_to`                                   | `true`         |

After changing, restart Thunderbird. New mail in Updates / Social /
Forums / Important will now ping the system the same way Inbox does.

## 3) Build the "All Mail" view

Thunderbird does not have a server-side "All Mail" folder by default
(that's a Gmail-ism). Two ways to get the same effect:

### Option A — Saved Search Folder (recommended)

`File → New → Saved Search Folder…`

- **Name**: `All Mail`
- **Create as a subfolder of**: top of the @plausiden.com account
- **Search messages in**: tick every folder listed under the account
  (use **Choose…** and select all twelve mailboxes), and tick
  **"Include subfolders"**
- **Match all of the following**: choose **"Match all messages"**
- Click **OK**

A virtual folder called `All Mail` appears at the top of the account
and shows every message regardless of category. It is searchable,
threaded, and sortable, and is computed locally from the cache so it's
instant.

### Option B — Unified Folders mode

`View → Folders → Unified`

This collapses every account's Inbox into one synthetic Inbox view.
Useful for multi-account workflows; less useful for a single-account
"All Mail" view (Option A is better there).

You can run both — they're independent.

## 4) Make the "Sent" folder land on the server, not on disk

If you can't drag a message into Sent (or if "Sent" appears greyed
out): Account Settings → **Copies & Folders**:

- **When sending messages, automatically place a copy in:**
  → tick **"Other"** → choose `Sent` under the @plausiden.com account
  (NOT "Local Folders → Sent").
- Same fix for **Drafts**, **Templates**, **Archives**, **Junk**.

This is the most common cause of "I can't move messages to <X>" — the
client thinks the destination lives in `Local Folders` (a local-only
profile-disk store) rather than under the account on the server.

## 5) If drag-and-drop move still fails

1. **Right-click the destination folder → Properties → General Information**
   confirm the folder is listed as on the IMAP server, not under
   "Local Folders".
2. **Right-click the destination folder → Properties → Repair Folder**
   forces a re-sync of folder metadata; clears stale ACL caches.
3. **File → Compact All Folders** — purges expunged copies that can
   block IMAP `COPY` operations.

If a move still fails, the dovecot log will show why:

```
sudo journalctl -u dovecot -f | grep -i copy
```

Look for `Permission denied`, `Mailbox doesn't exist`, or
`Quota exceeded`.

## 6) When new categories are added server-side

When `mail-admin emit-mailboxes` adds a new category folder
(e.g. `Trips`, `Receipts`), the laptop won't show it until you
re-subscribe:

1. Right-click the account → Subscribe…
2. Click **Refresh** in the dialog
3. Tick the new folder, OK

That's it.
