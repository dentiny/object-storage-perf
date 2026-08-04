#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="${BENCHMARK_IMAGE:?Set BENCHMARK_IMAGE to an immutable OCIR image reference}"
endpoint="${OSP_ENDPOINT:?Set OSP_ENDPOINT to the S3-compatible endpoint}"
bucket="${OSP_BUCKET:?Set OSP_BUCKET to the writable benchmark bucket}"
region="${OSP_REGION:-us-east-1}"
prefix="${OSP_PREFIX:-object-storage-perf}"
read_duration="${OSP_READ_DURATION:-120}"
write_duration="${OSP_WRITE_DURATION:-120}"
stat_duration="${OSP_STAT_DURATION:-30}"
read_concurrency="${OSP_READ_CONCURRENCY:-64}"
write_concurrency="${OSP_WRITE_CONCURRENCY:-128}"
stat_concurrency="${OSP_STAT_CONCURRENCY:-4}"
retry_max_times="${OSP_RETRY_MAX_TIMES:-3}"
timeout_seconds="${OSP_TIMEOUT_SECONDS:-30}"
io_timeout_seconds="${OSP_IO_TIMEOUT_SECONDS:-30}"
namespace="${NAMESPACE:-default}"
image_pull_secret="${IMAGE_PULL_SECRET:-ocir-secret}"
node_type="${NODE_TYPE:-BM.GPU.B4.8}"
manifest="$(mktemp)"
trap 'rm -f "$manifest"' EXIT

for value in \
  "$read_duration" \
  "$write_duration" \
  "$stat_duration" \
  "$read_concurrency" \
  "$write_concurrency" \
  "$stat_concurrency" \
  "$retry_max_times" \
  "$timeout_seconds" \
  "$io_timeout_seconds"; do
  if ! [[ "$value" =~ ^[1-9][0-9]*$ ]]; then
    echo "Duration and concurrency values must be positive integers" >&2
    exit 1
  fi
done

escape_sed() {
  printf '%s' "$1" | sed 's/[&|]/\\&/g'
}

kubectl -n "$namespace" get secret object-storage-perf-s3 >/dev/null
kubectl -n "$namespace" get secret "$image_pull_secret" >/dev/null

sed \
  -e "s|__NAMESPACE__|$(escape_sed "$namespace")|g" \
  -e "s|__BENCHMARK_IMAGE__|$(escape_sed "$image")|g" \
  -e "s|__IMAGE_PULL_SECRET__|$(escape_sed "$image_pull_secret")|g" \
  -e "s|__NODE_TYPE__|$(escape_sed "$node_type")|g" \
  -e "s|__OSP_ENDPOINT__|$(escape_sed "$endpoint")|g" \
  -e "s|__OSP_BUCKET__|$(escape_sed "$bucket")|g" \
  -e "s|__OSP_REGION__|$(escape_sed "$region")|g" \
  -e "s|__OSP_PREFIX__|$(escape_sed "$prefix")|g" \
  -e "s|__OSP_READ_DURATION__|$read_duration|g" \
  -e "s|__OSP_WRITE_DURATION__|$write_duration|g" \
  -e "s|__OSP_STAT_DURATION__|$stat_duration|g" \
  -e "s|__OSP_READ_CONCURRENCY__|$read_concurrency|g" \
  -e "s|__OSP_WRITE_CONCURRENCY__|$write_concurrency|g" \
  -e "s|__OSP_STAT_CONCURRENCY__|$stat_concurrency|g" \
  -e "s|__OSP_RETRY_MAX_TIMES__|$retry_max_times|g" \
  -e "s|__OSP_TIMEOUT_SECONDS__|$timeout_seconds|g" \
  -e "s|__OSP_IO_TIMEOUT_SECONDS__|$io_timeout_seconds|g" \
  "$root/k8s/benchmark-job.yaml" >"$manifest"

kubectl -n "$namespace" delete job object-storage-perf --ignore-not-found --wait=true
kubectl apply -f "$manifest"

printf 'Follow logs with: kubectl -n %q logs -f job/object-storage-perf\n' "$namespace"
