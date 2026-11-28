#!/bin/bash
# instructions retired (macOS /usr/bin/time -l), best of 3
F="$(cd "$(dirname "$0")/../.." && pwd)/target/release/flux"
for f in "$@"; do
  best=""
  for i in 1 2 3; do
    n=$(/usr/bin/time -l $F run --quiet --vm --no-cache $f 2>&1 >/dev/null | awk '/instructions retired/ {print $1}')
    if [ -z "$best" ] || [ "$n" -lt "$best" ]; then best=$n; fi
  done
  echo "$f $best"
done
