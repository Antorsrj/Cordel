#!/usr/bin/env bash
# Read-only Azure Cloud Shell inventory for planning Cordel's test environment.
# Prints resource names and billing metadata, never credentials or access tokens.
set -euo pipefail

subscription_id="f77cc05e-0316-4c39-9ee2-74b5d9078f0a"
region="eastus"

for tool in az jq; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'Required command is missing: %s\n' "$tool" >&2
    exit 1
  fi
done

subscriptions="$(az account list --all --output json)"
matches="$(jq --arg id "$subscription_id" '[.[] | select(.id == $id)] | length' <<<"$subscriptions")"
if [[ "$matches" != 1 ]]; then
  printf 'Expected exactly one accessible subscription with ID %s; found %s.\n' "$subscription_id" "$matches" >&2
  exit 1
fi

printf 'SUBSCRIPTION\n'
jq -r --arg id "$subscription_id" '.[] | select(.id == $id) | "name=\(.name)\nid=\(.id)\nstate=\(.state)"' <<<"$subscriptions"

printf '\nRESOURCE GROUPS\n'
az group list --subscription "$subscription_id" --output json \
  | jq -r 'if length == 0 then "(none)" else .[] | "\(.name)\t\(.location)" end'

printf '\nRESOURCES (first 100, count shown separately)\n'
resources="$(az resource list --subscription "$subscription_id" --output json)"
jq -r '"count=\(length)", (.[0:100][] | "\(.resourceGroup)\t\(.type)\t\(.name)\t\(.location)")' <<<"$resources"

printf '\nEAST US COMPUTE QUOTA\n'
if quota="$(az vm list-usage --location "$region" --subscription "$subscription_id" --output json 2>/dev/null)"; then
  jq -r '[.[] | select((.name.value // "" | ascii_downcase | contains("dsv5")) or (.name.value // "" | ascii_downcase | contains("totalregionalvcpus")))] | if length == 0 then "(no matching quota entries)" else .[] | "\(.name.value)\tcurrent=\(.currentValue)\tlimit=\(.limit)" end' <<<"$quota"
else
  printf '(unavailable with current permissions)\n'
fi

printf '\nSUBSCRIPTION BUDGETS\n'
if budgets="$(az consumption budget list --subscription "$subscription_id" --output json 2>/dev/null)"; then
  jq -r 'if length == 0 then "(none)" else .[] | "\(.name)\tamount=\(.amount)\ttimeGrain=\(.timeGrain)" end' <<<"$budgets"
else
  printf '(unavailable with current permissions)\n'
fi

printf '\nThis report is read-only. Resource names and the subscription ID are included; review before sharing.\n'
