#!/usr/bin/env bash

# Resolve components in filesystem order: a symlink must be followed before '..'.
# Missing suffixes are retained without creating any directories.
physical_profile_path() {
  local input="${1-}" current=/ rest component candidate
  if [[ "$input" != /* || "$input" == *$'\r'* || "$input" == *$'\n'* ]]; then
    printf 'desktop pet development: profile paths must be absolute and contain no CR/LF\n' >&2
    return 1
  fi
  rest="${input#/}"
  while [[ -n "$rest" ]]; do
    component="${rest%%/*}"
    if [[ "$rest" == */* ]]; then rest="${rest#*/}"; else rest=""; fi
    case "$component" in
      ''|.) continue ;;
      ..) current="${current%/*}"; current="${current:-/}"; continue ;;
    esac
    candidate="${current%/}/$component"
    if [[ -L "$candidate" || -e "$candidate" ]]; then
      if [[ ! -d "$candidate" ]]; then
        printf 'desktop pet development: profile path contains an unsafe directory component: %s\n' "$candidate" >&2
        return 1
      fi
      # Preserve trailing newlines from a symlink target until validation.
      current="$(CDPATH= cd -P -- "$candidate" && pwd -P && printf '.')" || return 1
      current="${current%.}"
      current="${current%$'\n'}"
      if [[ "$current" == *$'\r'* || "$current" == *$'\n'* || ! -x "$current" ]]; then
        printf 'desktop pet development: profile path is not an accessible CR/LF-free directory\n' >&2
        return 1
      fi
    else
      current="$candidate"
    fi
  done
  printf '%s\n' "$current"
}

require_separate_development_profile() {
  local config state default_config default_state
  config="$(physical_profile_path "${1-}")" || return 1
  state="$(physical_profile_path "${2-}")" || return 1
  default_config="$(physical_profile_path "${3-}")" || return 1
  default_state="$(physical_profile_path "${4-}")" || return 1
  if [[ "$config" == "$default_config" || "$state" == "$default_state" ]]; then
    printf 'desktop pet development: development must use separate config and state directories, not the shared default profile\n' >&2
    return 1
  fi
}
