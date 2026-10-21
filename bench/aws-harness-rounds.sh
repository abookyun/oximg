#!/usr/bin/env bash
# imgproxy's harness on c7i.large / c7g.large, as run for #63
# (BENCH.md, "Official harness on real AWS hardware"). Run on an
# instance from the harness's own CloudFormation template, after its
# user data has prepared the dataset, with this repo's
# bench/image-servers-benchmark.patch copied to ~.
#
#   Phase A: the full table, 4 servers x jpeg/png/webp/avif, 5 min at
#            the harness default -u $(nproc), oximg $NEW. Server order
#            rotates per format.
#   Phase B: JPEG, oximg $NEW / $OLD / imgproxy, 3 rounds x {2, 8} VUs,
#            order rotated every round.
#
# The instance's 8 GB root disk fits the images and the dataset, but
# not thumbor's build on top of a pulled dataset.zip, so the zip goes.
set -uo pipefail
NEW=${NEW:-0.15.0}
OLD=${OLD:-0.14.0}
cd ~/benchmark
OUT=~/out63; mkdir -p $OUT
exec > >(tee -a $OUT/run.log) 2>&1

if ! grep -q oximg docker-compose.yml; then
  git apply ~/image-servers-benchmark.patch
  # VUS override for phase B; unset keeps the harness's -u $(nproc).
  sed -i 's/run -u \$\$(nproc)/run -u $${VUS:-$$(nproc)}/' Makefile
  # python:3-slim is Python 3.15, where pillow-avif-plugin fails to build.
  sed -i 's/^FROM python:3-slim$/FROM python:3.13-slim/' thumbor/Dockerfile
fi
rm -f dataset.zip

for t in $NEW $OLD; do docker pull -q oximg/oximg:$t; done
docker pull -q darthsim/imgproxy:latest
docker pull -q ghcr.io/cshum/imagor:latest
{
  echo "host $(hostname) $(uname -m) $(nproc) cpus"
  grep -m1 'model name' /proc/cpuinfo || lscpu | grep -m1 'Model name'
  for i in oximg/oximg:$NEW oximg/oximg:$OLD darthsim/imgproxy:latest ghcr.io/cshum/imagor:latest; do
    echo "$i $(docker image inspect -f '{{index .RepoDigests 0}}' $i)"
  done
} > $OUT/versions.txt
cat $OUT/versions.txt

oximg_as() { docker tag oximg/oximg:$1 oximg:bench; }

cell() {  # label tool format [VUS]
  local label=$1 tool=$2 fmt=$3
  echo "=== $label $(date -u +%H:%M:%S)"
  if [ "$tool" = oximg ]; then
    VUS=${4:-} make up k6 down TOOL=oximg FORMAT=$fmt COMPOSE_ARGS="-d --wait" > $OUT/$label.log 2>&1
  else
    VUS=${4:-} make bench-$tool FORMAT=$fmt > $OUT/$label.log 2>&1
  fi
  grep -E "checks_succeeded|http_reqs" $OUT/$label.log | head -2
}

oximg_as $NEW
tools=(oximg imgproxy imagor thumbor)
i=0
for fmt in jpeg png webp avif; do
  for k in 0 1 2 3; do
    cell "A-$fmt-${tools[$(( (k + i) % 4 ))]}" ${tools[$(( (k + i) % 4 ))]} $fmt
  done
  i=$((i + 1))
done

configs=("ox-$NEW" "ox-$OLD" "imgproxy")
for r in 1 2 3; do
  for vus in 2 8; do
    for k in 0 1 2; do
      c=${configs[$(( (k + r) % 3 ))]}
      case $c in
        ox-*) oximg_as ${c#ox-}; cell "B-r$r-vu$vus-$c" oximg jpeg $vus ;;
        imgproxy) cell "B-r$r-vu$vus-$c" imgproxy jpeg $vus ;;
      esac
    done
  done
done
echo "DONE $(date -u)"
