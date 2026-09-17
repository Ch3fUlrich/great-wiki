#!/usr/bin/env bash
# Build and push the three great-wiki images at the current HEAD, then PROVE they are in
# Harbor before saying so.
#
# Written after a deploy failed with "Pull the image from Harbor": the ad-hoc loop it
# replaced ran `docker push … | tail -1`, which reports TAIL's exit code, so every push had
# been refused with `unauthorized` — the login on this box had expired — and the loop
# printed PUSHED_ALL anyway. Three earlier deploys had succeeded by luck. Two rules follow:
#
#   1. `set -o pipefail`, and every push's real status is checked.
#   2. A push is not proof. `docker manifest inspect` against Harbor is, and it runs last.
#
# It also logs in first, as the same robot the deploy playbook uses, from the same file —
# the login in ~/.docker/config.json expires and nothing says so until a push fails.
# Push straight after each build: the fleet's prune-docker removes unpushed images.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
if [ -n "$(git status --porcelain)" ]; then
  echo "working tree is not clean; an image is built from a commit, not from whatever is on disk" >&2
  exit 1
fi
sha="$(git rev-parse --short HEAD)"
env_file="../Server/secrets-generated/server__cloud__harbor__.env"
registry="harbor.ohje.ooguy.com"
user="$(grep -m1 '^HARBOR_ROBOT_USER=' "$env_file" | cut -d= -f2-)"
grep -m1 '^HARBOR_ROBOT_SECRET=' "$env_file" | cut -d= -f2- \
  | docker login "$registry" -u "$user" --password-stdin >/dev/null
for image in gw-caddy gw-api gw-web; do
  ref="$registry/great-wiki/$image:$sha"
  docker build -q -f "docker/$image.Dockerfile" -t "$ref" . >/dev/null
  docker push "$ref" | tail -1
done
for image in gw-caddy gw-api gw-web; do
  docker manifest inspect "$registry/great-wiki/$image:$sha" >/dev/null \
    || { echo "NOT IN HARBOR: $image:$sha" >&2; exit 1; }
done
echo "in Harbor at $sha: gw-caddy gw-api gw-web — now re-pin all three in the Server compose and run template 36"
