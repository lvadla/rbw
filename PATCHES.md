# Local compatibility build

Version: **1.15.0-patched.1**. This branch tracks upstream rbw 1.15.0; it is not an upstream release.

## Changes

1. **Cipher create/update compatibility**: cherry-picked upstream PR [#386](https://github.com/doy/rbw/pull/386), retaining its author's attribution. Store the authenticated user ID from sync and send `encryptedFor` when creating or updating an item. An existing cache without the ID is synced before its first write.
2. **Session renewal**: retain optional renewed refresh tokens, not just access tokens, through blocking CLI requests and asynchronous agent syncs. Persist both after the successful retried operation. Preserve the old refresh token when a server omits a replacement. Folder operations also carry renewed credentials into subsequent requests.
3. **Errors**: handle non-success refresh responses before deserializing success JSON. Rejected sessions produce an actionable error; unrelated HTTP failures retain their status. No automatic cache purge or MFA bypass.
4. **Identification**: use the same `1.15.0-patched.1` version for CLI and agent so the version check detects an old agent.

The patch does not change cryptography, MFA policy, server settings, or token lifetimes. Token fields are `pub(crate)`, not unrestricted public fields. No token values should be printed or committed.

## Validation

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --all-features --locked
cargo build --release --locked
```

26 total tests pass (14 existing, 12 new). The new tests cover create/update request fields, blocking and async renewal/preservation/error handling, serialization of updated credentials, and old-cache compatibility. Clippy completes with warnings in unchanged upstream code; it is not warning-free on the current toolchain.

A live smoke test against Vaultwarden 1.37.4 used a separate local profile/cache: an invalid access token with a valid refresh token triggered renewal, and the new refresh token and user ID were verified on disk. A second test verified that rejected credentials yield the new error without deleting the cache. Temporary profile credentials were removed afterwards. These tests do not prove 30 days of continuous operation or test every concurrent CLI/agent scenario.

## Build and install locally

Keep the packaged `/usr/bin/rbw` and `/usr/bin/rbw-agent` installed for rollback. Back up local rbw state securely first; it contains sensitive session credentials and encrypted vault data.

Build as above, then stop the existing agent:

```sh
rbw stop-agent
```

Install **both** release binaries into a user-owned versioned directory and link them into `~/.local/bin`. Ensure that directory comes before `/usr/bin` in PATH. Alternatively, use explicit binary paths and set `RBW_AGENT` to the matching built agent. Restarting the agent clears its in-memory unlock keys; a master-password prompt on the next unlock is expected, not a server-session failure.

```sh
rbw --version
rbw-agent --version
rbw unlock
rbw sync
```

Do not use `rbw purge` just to switch versions.

## Rollback

Stop the patched agent, remove only the two `~/.local/bin` links installed for this build, refresh your shell's command cache (`hash -r` in bash or `rehash` in zsh), then run `/usr/bin/rbw unlock`. The old binaries can read the cache, ignoring the added `user_id` field. Rollback restores the old client bugs too.

Keep local credentials, vault exports, recovery copies, and deployment-specific information **outside this repository**. Rebase reviewed changes onto future upstream releases rather than replacing the entire project indiscriminately.
