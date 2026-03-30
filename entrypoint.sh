#!/bin/sh
# Fix ownership of /data volume files (may have been created as root)
chown -R metsuke:metsuke /data 2>/dev/null || true
exec su -s /bin/sh metsuke -c "metsuke"
