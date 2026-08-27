#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
node_type="${NODE_TYPE:-BM.GPU.B4.8}"

if ! [[ "$node_type" =~ ^[a-zA-Z0-9.-]+$ ]]; then
  echo "NODE_TYPE must contain only letters, numbers, dots, and hyphens" >&2
  exit 1
fi

: "${KUBE_CONTEXT:=oke-phx-gpu}"
: "${OSP_ENDPOINT:?Set OSP_ENDPOINT to the OCI S3-compatible endpoint}"
: "${OSP_BUCKET:?Set OSP_BUCKET to the writable OCI bucket}"
: "${OSP_REGION:=us-phoenix-1}"
: "${CPU_REQUEST:=4}"
: "${CPU_LIMIT:=64}"
: "${TOKIO_WORKER_THREADS:=64}"

BENCHMARK_MANIFEST="$script_dir/benchmark-job-oci.yaml"

export KUBE_CONTEXT OSP_ENDPOINT OSP_BUCKET OSP_REGION
export CPU_REQUEST CPU_LIMIT TOKIO_WORKER_THREADS BENCHMARK_MANIFEST

# shellcheck source=./_deploy-job.sh
source "$script_dir/_deploy-job.sh"
deploy_benchmark_job
