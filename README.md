# cythe

A lightweight CI program that uses docker to test and build your applications.

## Config

All configuration is located at `/etc/cythe`.

Configure allowed repos at `/etc/cythe/allowed-repos.json`.

```json
["darrkenn/cythe", "darrkenn/cythe-test"]
```

Place all secrets at `/etc/cythe/secrets` with this structure:

```
darrkenn/ # ORG NAME
  cythe.secret # REPO NAME
  cythe-test.secret # REPO NAME

```
