#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="${BENCHMARK_IMAGE:?Set BENCHMARK_IMAGE to an immutable OCIR image reference}"
endpoint="${OSP_ENDPOINT:?Set OSP_ENDPOINT to the S3-compatible endpoint}"
bucket="${OSP_BUCKET:?Set OSP_BUCKET to the writable benchmark bucket}"
region="${OSP_REGION:-us-east-1}"
prefix="${OSP_PREFIX:-object-storage-perf}"
duration="${OSP_DURATION:-10}"
concurrency="${OSP_CONCURRENCY:-4}"
multipart_concurrency="${OSP_MULTIPART_CONCURRENCY:-1}"
namespace="${NAMESPACE:-default}"
s3_secret="${S3_SECRET:-object-storage-perf-s3}"
image_pull_secret="${IMAGE_PULL_SECRET:-ocir-secret}"
manifest="$(mktemp)"
trap 'rm -f "$manifest"' EXIT

for value in "$duration" "$concurrency" "$multipart_concurrency"; do
  if ! [[ "$value" =~ ^[1-9][0-9]*$ ]]; then
    echo "Duration and concurrency values must be positive integers" >&2
    exit 1
  fi
done

escape_sed() {
  printf '%s' "$1" | sed 's/[&|]/\\&/g'
}

kubectl -n "$namespace" get secret "$s3_secret" >/dev/null
kubectl -n "$namespace" get secret "$image_pull_secret" >/dev/null

sed \
  -e "s|__NAMESPACE__|$(escape_sed "$namespace")|g" \
  -e "s|__BENCHMARK_IMAGE__|$(escape_sed "$image")|g" \
  -e "s|__IMAGE_PULL_SECRET__|$(escape_sed "$image_pull_secret")|g" \
  -e "s|__S3_SECRET__|$(escape_sed "$s3_secret")|g" \
  -e "s|__OSP_ENDPOINT__|$(escape_sed "$endpoint")|g" \
  -e "s|__OSP_BUCKET__|$(escape_sed "$bucket")|g" \
  -e "s|__OSP_REGION__|$(escape_sed "$region")|g" \
  -e "s|__OSP_PREFIX__|$(escape_sed "$prefix")|g" \
  -e "s|__OSP_DURATION__|$duration|g" \
  -e "s|__OSP_CONCURRENCY__|$concurrency|g" \
  -e "s|__OSP_MULTIPART_CONCURRENCY__|$multipart_concurrency|g" \
  "$root/k8s/benchmark-job.yaml" >"$manifest"

kubectl -n "$namespace" delete job object-storage-perf --ignore-not-found --wait=true
kubectl apply -f "$manifest"

printf 'Follow logs with: kubectl -n %q logs -f job/object-storage-perf\n' "$namespace"
