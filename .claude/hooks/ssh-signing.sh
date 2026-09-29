#!/bin/bash
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: Apache-2.0
#
# sessionstart: власний ssh-ключ для підпису комітів замість /tmp/code-sign
set -euo pipefail
[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0
[ -n "${GIT_SIGNING_KEY_B64:-}" ] || { echo "ssh-signing: GIT_SIGNING_KEY_B64 не задано, пропускаю" >&2; exit 0; }

command -v ssh-keygen >/dev/null || { apt-get update -qq || true; apt-get install -y -qq openssh-client; } >&2

dir="$HOME/.ssh"; key="$dir/signing_key"
mkdir -p "$dir" && chmod 700 "$dir"
printf %s "$GIT_SIGNING_KEY_B64" | base64 -d > "$key"
chmod 600 "$key"
ssh-keygen -y -f "$key" > "$key.pub"

email="${GIT_SIGNING_EMAIL:-$(git config --global user.email)}"
echo "$email $(cut -d' ' -f1,2 "$key.pub")" > "$dir/allowed_signers"

git config --global --unset-all gpg.ssh.program || true
git config --global gpg.format ssh
git config --global user.signingkey "$key.pub"
git config --global gpg.ssh.allowedSignersFile "$dir/allowed_signers"
git config --global user.email "$email"
git config --global commit.gpgsign true
