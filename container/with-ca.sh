#!/bin/sh
set -eu
if [ -s /run/secrets/ca ]; then
  export SSL_CERT_FILE=/run/secrets/ca CURL_CA_BUNDLE=/run/secrets/ca CARGO_HTTP_CAINFO=/run/secrets/ca \
    PIP_CERT=/run/secrets/ca GIT_SSL_CAINFO=/run/secrets/ca
fi
exec "$@"
