#!/bin/bash
# SEDRA (#13): stops the dry run of start.sh (only its own user scopes). --wipe also deletes its data directory.
DRY=${DRY:-/data/testnet-dryrun}
for u in $(systemctl --user list-units --type=scope --no-legend 'sedra-dryrun-*' | awk '{print $1}'); do
  systemctl --user stop "$u" && echo "stopped $u"
done
if [ "${1:-}" = "--wipe" ]; then
  case "$DRY" in
    */testnet-dryrun) rm -rf "$DRY" && echo "removed $DRY" ;;
    *) echo "refusing to wipe $DRY (not a */testnet-dryrun directory)" ;;
  esac
fi
