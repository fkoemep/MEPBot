#!/usr/bin/env bash

_SERVICE_ACCOUNT_NAME=$1
PROJECT_ID=$2
create=true

gcloud services enable artifactregistry.googleapis.com cloudbuild.googleapis.com run.googleapis.com cloudscheduler.googleapis.com iam.googleapis.com sourcerepo.googleapis.com cloudfunctions.googleapis.com storage.googleapis.com

for scopesInfo in $(gcloud iam service-accounts list --filter="displayName:${_SERVICE_ACCOUNT_NAME}" --format="csv[no-heading](displayName)")
do
      create=false
      break
done

if [ "$create" = true ] ; then
  echo "Creating service account ${_SERVICE_ACCOUNT_NAME}"
  gcloud iam service-accounts create "${_SERVICE_ACCOUNT_NAME}" --display-name="${_SERVICE_ACCOUNT_NAME}"
else
  echo "Service account already exists."
fi

echo "Setting roles for service account ${_SERVICE_ACCOUNT_NAME}"
gcloud projects add-iam-policy-binding "${PROJECT_ID}" --member="serviceAccount:${_SERVICE_ACCOUNT_NAME}@${PROJECT_ID}.iam.gserviceaccount.com" --role="roles/cloudbuild.serviceAgent" --condition=None --quiet

gcloud projects add-iam-policy-binding "${PROJECT_ID}" --member="serviceAccount:${_SERVICE_ACCOUNT_NAME}@${PROJECT_ID}.iam.gserviceaccount.com" --role="roles/run.invoker" --condition=None --quiet

gcloud projects add-iam-policy-binding "${PROJECT_ID}" --member="serviceAccount:${_SERVICE_ACCOUNT_NAME}@${PROJECT_ID}.iam.gserviceaccount.com" --role="roles/cloudscheduler.admin" --condition=None --quiet

gcloud projects add-iam-policy-binding "${PROJECT_ID}" --member="serviceAccount:${_SERVICE_ACCOUNT_NAME}@${PROJECT_ID}.iam.gserviceaccount.com" --role="roles/iam.serviceAccountUser" --condition=None --quiet

gcloud projects add-iam-policy-binding "${PROJECT_ID}" --member="serviceAccount:${_SERVICE_ACCOUNT_NAME}@${PROJECT_ID}.iam.gserviceaccount.com" --role="roles/storage.admin" --condition=None --quiet
