#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
node_name="${NODE_NAME:?Set NODE_NAME to a Crusoe worker in eu-norway1-a}"

if ! [[ "$node_name" =~ ^[a-zA-Z0-9.-]+$ ]]; then
  echo "NODE_NAME must be a valid Kubernetes node name" >&2
  exit 1
fi

: "${KUBE_CONTEXT:=hark-norway-gpu}"
: "${OSP_ENDPOINT:=https://object.eu-norway1-a.crusoecloudcompute.com}"
: "${OSP_BUCKET:=hjiang-test-bucket}"
: "${OSP_REGION:=eu-norway1-a}"
: "${CPU_REQUEST:=4}"
: "${CPU_LIMIT:=64}"
: "${TOKIO_WORKER_THREADS:=64}"

BENCHMARK_MANIFEST="$script_dir/benchmark-job-crusoe.yaml"

export KUBE_CONTEXT OSP_ENDPOINT OSP_BUCKET OSP_REGION
export CPU_REQUEST CPU_LIMIT TOKIO_WORKER_THREADS BENCHMARK_MANIFEST

# shellcheck source=./_deploy-job.sh
source "$script_dir/_deploy-job.sh"
deploy_benchmark_job
