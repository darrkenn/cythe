# cythe

A lightweight CI program that uses docker to test and build your applications.

## Config

All configuration is located at `/etc/cythe`.

Configure which repositories cythe will track in `/etc/cythe/repos.toml`.

```toml
[[repo]]
name = "darrkenn/cythe"
tracked_branch = "main"
url = "https://github.com/darrkenn/cythe"

[[repo]]
name = "darrkenn/cythe-test"
tracked_branch = "main"
url = "https://github.com/darrkenn/cythe-test"
```

Place all repository secrets at `/etc/cythe/secrets` with this structure.

```sh
darrkenn/ # Organisation name
cythe.secret # Repository name
cythe-test.secret # Repository name
```

Configure cythe's runtime options at `/etc/cythe/config.toml`.

```toml
# Whether or not to cache container images between runs
cache_images = true
# Maximum number of concurrent runners
# Max is 255 but 4 is recommended
max_active_runners = 4
# Logging level
# Options: "error", "warn", "info", "trace"
log_level = "info"
# Whether or not to continue executing steps if one fails
continue_on_fail = false
```
