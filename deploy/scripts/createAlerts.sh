#!/usr/bin/env bash
# This script adds alerts .

_JOB_NAME=$1
_EMAIL_ADDRESS=$2
_CHANNEL_ID=""
create_channel=true
create_alert=true

# BEGIN MONITORING CHANNELS
list_channels () {
  for scopesInfo in $(gcloud beta monitoring channels list --filter="labels.email_address:'${_EMAIL_ADDRESS}' AND type:'email' AND displayName:'${_JOB_NAME}'" --format='csv[no-heading](name)')
  do
    create_channel=false
    _CHANNEL_ID=$scopesInfo
    break
  done
}

list_channels

if [ "$create_channel" = true ] ; then
  echo "Creating monitoring channel ${_JOB_NAME} with email address ${_EMAIL_ADDRESS}"
  gcloud beta monitoring channels create --display-name="${_JOB_NAME}" --type=email --channel-labels=email_address="${_EMAIL_ADDRESS}"
  list_channels
else
  echo "Monitoring channel already exists."
fi
# END MONITORING CHANNELS

# BEGIN MONITORING POLICIES
for scopesInfo in $(gcloud alpha monitoring policies list --filter="displayName:'${_JOB_NAME}'" --format='csv[no-heading](name)')
do
  create_alert=false
  break
done

cat << EOF > alertspolicy.json
{"combiner":"OR","alertStrategy":{"notificationRateLimit":{"period": "300s"}},"conditions":[{"displayName":"Cloud Services with errors","conditionMatchedLog":{"filter":"(resource.type=\"build\" OR resource.type=\"cloud_run_revision\") AND (severity = \"ERROR\" OR severity=\"EMERGENCY\" OR severity=\"CRITICAL\")"}}]}
EOF

if [ "$create_alert" = true ] ; then
  echo "Creating monitoring policy ${_JOB_NAME} with channel id ${_CHANNEL_ID}"
  gcloud alpha monitoring policies create --display-name="${_JOB_NAME}" --notification-channels="${_CHANNEL_ID}" --policy-from-file=alertspolicy.json
else
  echo "Monitoring policy already exists."
fi
# END MONITORING POLICIES