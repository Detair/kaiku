#!/bin/sh
# Entrypoint of the Kaiku web image: render the site config into /tmp (the
# root filesystem may be read-only) and run nginx in the foreground.
set -eu

# KAIKU_MEDIA_ORIGINS goes into the Content-Security-Policy header, so accept
# only a space-separated list of http(s) origins (no paths, quotes or ';').
media="${KAIKU_MEDIA_ORIGINS:-}"
for origin in $media; do
    if ! printf '%s\n' "$origin" | grep -Eq '^https?://[A-Za-z0-9.-]+(:[0-9]{1,5})?$'; then
        echo "kaiku-web: KAIKU_MEDIA_ORIGINS entry '$origin' is not an origin like https://files.example.com" >&2
        exit 1
    fi
done
# Collapse whitespace so the header stays on one line.
KAIKU_MEDIA_ORIGINS="$(printf '%s' "$media" | tr -s ' \t\n' ' ' | sed 's/^ //; s/ $//')"
export KAIKU_MEDIA_ORIGINS

# Listen on IPv6 too, unless the host has it disabled (nginx would refuse
# to start on [::]:8080).
if [ -e /proc/net/if_inet6 ]; then
    KAIKU_LISTEN_V6="listen [::]:8080;"
else
    KAIKU_LISTEN_V6=""
fi
export KAIKU_LISTEN_V6

mkdir -p /tmp/nginx
envsubst '${KAIKU_MEDIA_ORIGINS} ${KAIKU_LISTEN_V6}' \
    < /etc/nginx/kaiku/kaiku.conf.template \
    > /tmp/nginx/kaiku.conf

exec nginx -g 'daemon off;'
