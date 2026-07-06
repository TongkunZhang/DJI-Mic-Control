#!/bin/sh
# Runs after the .deb/.rpm is removed.
set -e

# Reload udev so the removed USB access rule stops applying.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules >/dev/null 2>&1 || true
fi

exit 0
