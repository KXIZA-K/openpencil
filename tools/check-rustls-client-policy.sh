#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

scan_roots=(crates vendor/anthropic-agent-sdk)

if rg -n 'reqwest::(blocking::)?Client::(new|default)\(\)' "${scan_roots[@]}" -g '*.rs'; then
  echo "error: reqwest Client::new/default would inherit SDK-native TLS; use an explicit rustls builder" >&2
  exit 1
fi

if rg -U -n --pcre2 \
  'reqwest::(blocking::)?Client::builder\(\)(?!\s*\.use_rustls_tls)' \
  "${scan_roots[@]}" -g '*.rs'; then
  echo "error: reqwest builder must call use_rustls_tls() explicitly" >&2
  exit 1
fi

for file in \
  crates/op-collab-host/src/jwks.rs \
  crates/op-collab-host/src/runtime/relay_bootstrap.rs \
  crates/op-collab-relay-control-plane/src/http_client.rs; do
  if ! rg -U -q 'Client::builder\(\)\s*\.use_rustls_tls\(\)' "$file"; then
    echo "error: $file must pin its aliased reqwest Client builder to rustls" >&2
    exit 1
  fi
done

if rg -n 'connect_async(_with_config)?\(' \
  crates/op-acp/src/client.rs crates/op-collab-relay-client/src/session.rs; then
  echo "error: WebSocket clients must pass an explicit rustls Connector" >&2
  exit 1
fi

for file in crates/op-acp/src/client.rs crates/op-collab-relay-client/src/session.rs; do
  if ! rg -q 'connect_async_tls_with_config' "$file"; then
    echo "error: $file is missing the explicit TLS connector path" >&2
    exit 1
  fi
done

echo "rustls client policy: ok"
