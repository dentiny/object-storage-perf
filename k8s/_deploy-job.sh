#!/usr/bin/env bash

deploy_benchmark_job() (
  local image endpoint bucket region prefix
  local read_duration write_duration stat_duration
  local read_concurrency write_concurrency stat_concurrency
  local retry_max_times timeout_seconds io_timeout_seconds
  local namespace image_pull_secret kube_context
  local cpu_request cpu_limit tokio_worker_threads benchmark_manifest
  local node_name node_type
  local manifest value

  image="${BENCHMARK_IMAGE:?Set BENCHMARK_IMAGE to an immutable image reference}"
  endpoint="${OSP_ENDPOINT:?Set OSP_ENDPOINT to the S3-compatible endpoint}"
  bucket="${OSP_BUCKET:?Set OSP_BUCKET to the writable benchmark bucket}"
  region="${OSP_REGION:?Set OSP_REGION to the signing region}"
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
  kube_context="${KUBE_CONTEXT:?Set KUBE_CONTEXT to the target cluster}"
  cpu_request="${CPU_REQUEST:-4}"
  cpu_limit="${CPU_LIMIT:-64}"
  tokio_worker_threads="${TOKIO_WORKER_THREADS:-64}"
  benchmark_manifest="${BENCHMARK_MANIFEST:?Set BENCHMARK_MANIFEST for the target platform}"
  node_name="${NODE_NAME:-}"
  node_type="${NODE_TYPE:-}"

  for value in \
    "$read_duration" \
    "$write_duration" \
    "$stat_duration" \
    "$read_concurrency" \
    "$write_concurrency" \
    "$stat_concurrency" \
    "$retry_max_times" \
    "$timeout_seconds" \
    "$io_timeout_seconds" \
    "$cpu_request" \
    "$cpu_limit" \
    "$tokio_worker_threads"; do
    if ! [[ "$value" =~ ^[1-9][0-9]*$ ]]; then
      echo "Durations, concurrency, CPU, and worker values must be positive integers" >&2
      return 1
    fi
  done

  escape_sed() {
    printf '%s' "$1" | sed 's/[&|]/\\&/g'
  }

  manifest="$(mktemp)"
  trap 'rm -f "$manifest"' EXIT

  kubectl --context "$kube_context" -n "$namespace" \
    get secret object-storage-perf-s3 >/dev/null
  kubectl --context "$kube_context" -n "$namespace" \
    get secret "$image_pull_secret" >/dev/null

  sed \
    -e "s|__NAMESPACE__|$(escape_sed "$namespace")|g" \
    -e "s|__BENCHMARK_IMAGE__|$(escape_sed "$image")|g" \
    -e "s|__IMAGE_PULL_SECRET__|$(escape_sed "$image_pull_secret")|g" \
    -e "s|__NODE_NAME__|$(escape_sed "$node_name")|g" \
    -e "s|__NODE_TYPE__|$(escape_sed "$node_type")|g" \
    -e "s|__CPU_REQUEST__|$cpu_request|g" \
    -e "s|__CPU_LIMIT__|$cpu_limit|g" \
    -e "s|__TOKIO_WORKER_THREADS__|$tokio_worker_threads|g" \
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
    "$benchmark_manifest" >"$manifest"

  kubectl --context "$kube_context" -n "$namespace" \
    delete job object-storage-perf --ignore-not-found --wait=true
  kubectl --context "$kube_context" apply --filename "$manifest"

  printf 'Follow logs with: kubectl --context %q -n %q logs -f job/object-storage-perf\n' \
    "$kube_context" "$namespace"
)
