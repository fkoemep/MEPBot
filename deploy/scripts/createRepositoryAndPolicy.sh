#!/usr/bin/env bash
# This script sets a cleanup policy for the gcr.io repository so that only the most recent version is kept.

_JOB_REGION=$1
_ARTIFACT_REPO_NAME=$2
create=true

for scopesInfo in $(gcloud artifacts repositories list --filter="format:DOCKER AND name:${_ARTIFACT_REPO_NAME}" --format='csv[no-heading](name)' --location="${_JOB_REGION}")
do
  create=false
  break
done

if [ "$create" = true ] ; then
  echo "Creating repository ${_ARTIFACT_REPO_NAME} in location ${_JOB_REGION}"
    gcloud artifacts repositories create --location="${_JOB_REGION}" "${_ARTIFACT_REPO_NAME}" --repository-format=docker --mode=standard-repository
else
  echo "Repository already exists."
fi

cat << EOF > artifactspolicy.json
[{"name": "keep-minimum-versions", "action": {"type": "Keep"}, "mostRecentVersions": {"keepCount": 1}}, {"name": "delete-everything", "action": {"type": "Delete"},"condition": {"tagState": "any"}}]
EOF

gcloud artifacts repositories set-cleanup-policies "${_ARTIFACT_REPO_NAME}" --policy=artifactspolicy.json --location="${_JOB_REGION}"

echo "Policy set for ${_ARTIFACT_REPO_NAME} repository"