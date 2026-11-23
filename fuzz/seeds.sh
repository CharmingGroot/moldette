#!/bin/sh
# Writes each line of the end-to-end test data as one seed input per target.
set -eu
cd "$(dirname "$0")"
data=../crates/moldette/tests/data

seed() {
    target=$1
    mkdir -p "corpus/$target"
    n=0
    while IFS= read -r line; do
        n=$((n + 1))
        printf '%s' "$line" > "corpus/$target/seed-$n"
    done
}

seed json < "$data/order_api.ndjson"
seed delimited < "$data/order_export.csv"
seed regex < "$data/nginx_access.log"
printf '%s\n' 'id=A-1 total=12.50 note=gift' 'id=A-2 total= note' '=x id==' | seed kv
printf '%s\n' 'A-1|web|{"sku":"S-9","items":"B-7;C-3"}' 'A-2|app|{"sku":"S-1"}' 'A-3|web|not json' | seed bodies
cat "$data/order_api.ndjson" "$data/order_export.csv" "$data/nginx_access.log" | seed pipeline
mkdir -p corpus/framing
cp "$data/order_api.ndjson" corpus/framing/ndjson
cp "$data/nginx_access.log" corpus/framing/lines
printf '%s' '[{"id":"A-1"}, "x", 2]' > corpus/framing/array
