#!/bin/sh
# Debian post-removal script.
#
# Only acts on `purge`, never plain `remove`: removing a package should not
# throw away the user's session and settings, but purging is an explicit request
# to leave nothing behind.
#
# The files live in the *user's* home, not root's, so resolve the person who
# invoked the removal rather than trusting $HOME.
set -e

case "$1" in
  purge) ;;
  *) exit 0 ;;
esac

APP_ID="com.ankurk91.google-chat-tauri"

if [ -n "$SUDO_USER" ]; then
    USER_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
else
    USER_HOME="$HOME"
fi

[ -n "$USER_HOME" ] && [ -d "$USER_HOME" ] || exit 0

# Session (cookies, local storage) and the logs
rm -rf "$USER_HOME/.local/share/$APP_ID"
# Preferences and window geometry
rm -rf "$USER_HOME/.config/$APP_ID"
# The webview's cache. Kept in step with `data_dirs` in features/reset.rs,
# which wipes these same three -- a purge that left one behind would be a
# weaker reset than the in-app one.
rm -rf "$USER_HOME/.cache/$APP_ID"
# Launch-at-login entry, if the user enabled it
rm -f "$USER_HOME/.config/autostart/Google Chat.desktop"
rm -f "$USER_HOME/.config/autostart/google-chat-tauri.desktop"

exit 0
