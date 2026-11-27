#!/usr/bin/env bash
set -euo pipefail

readonly repository=it00021hot/hongguo-desktop
readonly github_repository="github.com/$repository"
readonly app_config=crates/hongguo-nsh/nsh.json
readonly windows_triple=x86_64-pc-windows-msvc
readonly inno_version=6.7.4
readonly inno_sha=a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c
readonly translation_commit=6ef32198ef1f7b7b375cd4b6b90896c2a58eb4c2
readonly translation_sha=e0b0b350e2245f3c5e65586dfe43d574f6e7f06f2261149aba284954b3fc9a8d
readonly wine_release=11.0_1
readonly wine_sha=b50dc50ec7f41d58b115a6b685d4d1315ba3c797bd3aa0f49213f2703cb82388
readonly legacy_sha=d36bac7c56fc7b5c25e72c14aa7faa8ee661cd8a88cf039f1f1cc64815a44fe9

usage() {
    printf 'Usage: %s <version> [--build-only]\n' "$0"
}

fail() {
    printf '%s\n' "$*" >&2
    exit 1
}

main() {
    if [[ ${1:-} == --help || ${1:-} == -h ]]; then
        usage
        return
    fi
    [[ $# == 1 || ( $# == 2 && $2 == --build-only ) ]] || fail "$(usage)"
    local version="${1#v}" build_only="${2:-}"
    [[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "Invalid stable release version: $1"
    local tag="v$version" tag_ref="refs/tags/v$version"

    [[ $(uname -s) == Darwin ]] || fail "Local desktop releases require macOS"
    local command
    for command in git gh jq cargo rustup curl shasum xcrun; do
        command -v "$command" >/dev/null || fail "Missing command: $command"
    done
    gh auth status --hostname github.com

    local root
    root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
    cd "$root"
    local release_root="$root/target/local-release"
    mkdir -p "$release_root"
    release_lock="$release_root/lock"
    mkdir "$release_lock" || fail "Another local release is using $release_root"
    trap 'rmdir "$release_lock"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    if [[ -z $build_only ]]; then
        verify_origin
    fi
    git fetch --no-tags origin '+refs/heads/main:refs/remotes/origin/main'
    local main_commit source_commit draft='' remote_tag=''
    main_commit=$(git rev-parse refs/remotes/origin/main)
    source_commit=$main_commit
    if [[ -z $build_only ]]; then
        draft=$(release_draft "$tag")
        if [[ -n $draft ]] && ! jq -e '.draft' <<<"$draft" >/dev/null; then
            fail "Release $tag is already published"
        fi
        remote_tag=$(tag_commit "$tag_ref")
        source_commit=$(release_commit "$tag_ref" "$main_commit" "$remote_tag")
        verify_manual_workflow "$main_commit"
        verify_manual_workflow "$source_commit"
    fi
    local marker="<!-- hongguo-local-release:$source_commit -->"
    if [[ -n $draft ]]; then
        jq -e --arg marker "$marker" '.draft and (.body | sub("[[:space:]]+$"; "") | endswith($marker))' <<<"$draft" >/dev/null ||
            fail "Release $tag already exists. Only a draft created by this script for $source_commit can resume."
    fi

    local source="$release_root/source"
    prepare_worktree "$source" "$source_commit"
    cd "$source"
    [[ $(jq -er '.version' "$app_config") == "$version" ]] ||
        fail "$source_commit does not contain app version $version. Merge the release metadata and changelog into main first."

    local output
    output=$(mktemp -d "$release_root/$tag.XXXXXXXX")
    printf 'Building %s at %s in %s\n' "$tag" "$source_commit" "$output"
    local assets="$output/assets" notes="$output/release-notes.md"
    mkdir "$assets"
    cargo fmt -- --check
    cargo build-tool enhancement-assets
    cargo build-tool theme-check
    cargo build-tool release-preflight --notes "$notes"
    prepare_release_tools "$release_root"
    build_desktop "$output"
    cargo build-tool release-preflight --artifacts "$assets" --notes "$notes"
    gh release download v0.6.6 --repo "$github_repository" --pattern latest.json --dir "$assets"
    check_hash "$legacy_sha" "$assets/latest.json"
    printf 'Verified assets: %s\n' "$assets"
    if [[ -z $build_only ]]; then
        push_release_tag "$tag" "$source_commit" "$remote_tag"
        publish_release "$tag" "$draft" "$marker" "$output"
    fi
}

verify_origin() {
    local url
    for url in "$(git remote get-url origin)" "$(git remote get-url --push --all origin)"; do
        case "${url%.git}" in
            "git@github.com:$repository"|"ssh://git@github.com/$repository"|"https://github.com/$repository") ;;
            *) fail "origin must point to github.com/$repository" ;;
        esac
    done
}

release_draft() {
    gh api --hostname github.com --paginate "repos/$repository/releases?per_page=100" |
        jq -cs --arg tag "$1" '[.[][] | select(.tag_name == $tag)] |
            if length > 1 then error("Multiple releases for the tag") else .[] end'
}

tag_commit() {
    git ls-remote --tags origin "$1" "$1^{}" |
        awk 'NR == 1 {tag = $1} /\^\{\}$/ {tag = $1} END {if (tag != "") print tag}'
}

release_commit() {
    local ref=$1 main_commit=$2 remote=$3 local_commit=''
    if git show-ref --verify --quiet "$ref"; then
        local_commit=$(git rev-parse "$ref^{commit}")
    fi
    if [[ -n $remote ]]; then
        git fetch --no-tags origin "$ref" >&2
    fi
    [[ -z $remote || -z $local_commit || $remote == "$local_commit" ]] ||
        fail "Local and remote $ref point to different commits"
    local commit="${remote:-${local_commit:-$main_commit}}"
    git merge-base --is-ancestor "$commit" "$main_commit" ||
        fail "$ref points to $commit outside origin/main"
    printf '%s\n' "$commit"
}

verify_manual_workflow() {
    local workflow
    workflow=$(git show "$1:.github/workflows/release.yml")
    if [[ $workflow =~ (^|$'\n')[[:blank:]]*push: ]]; then
        fail "The release workflow at $1 still triggers on tag push. Merge the local release changes into main first."
    fi
}

prepare_worktree() {
    local source=$1 commit=$2 common_dir source_common_dir source_status
    if [[ ! -e $source ]]; then
        git worktree add --detach "$source" "$commit"
        return
    fi
    common_dir=$(git rev-parse --path-format=absolute --git-common-dir)
    [[ -f $source/.git ]] || fail "Invalid local release worktree: $source"
    source_common_dir=$(git -C "$source" rev-parse --path-format=absolute --git-common-dir)
    source_status=$(git -C "$source" status --porcelain)
    [[ $source_common_dir == "$common_dir" && -z $source_status ]] ||
        fail "Local release worktree has changes or belongs to a different repository: $source"
    git -C "$source" switch --detach "$commit"
}

prepare_release_tools() {
    local release_root=$1 target llvm_bin installed_targets
    xcrun --sdk macosx --show-sdk-path >/dev/null
    installed_targets=$(rustup target list --installed)
    for target in aarch64-apple-darwin x86_64-apple-darwin "$windows_triple"; do
        grep -Fx "$target" <<<"$installed_targets" >/dev/null ||
            fail "Install the missing Rust target with rustup target add $target"
    done
    cargo xwin --version >/dev/null || fail "Install cargo-xwin 0.23.1 before releasing"
    if [[ -n ${HONGGUO_LLVM_BIN:-} ]]; then
        export PATH="$HONGGUO_LLVM_BIN:$PATH"
    elif ! command -v llvm-rc >/dev/null && command -v brew >/dev/null; then
        llvm_bin=$(brew --prefix llvm)/bin
        export PATH="$llvm_bin:$PATH"
    fi
    for target in llvm-rc clang-cl lld-link; do
        command -v "$target" >/dev/null || fail "Add LLVM's bin directory to PATH or set HONGGUO_LLVM_BIN"
    done
    cargo build --locked --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli
    cargo build-tool cross-tools
    ln -sfn nsh-windows-rc target/nsh-cross-tools/rc.exe
    prepare_inno "$release_root"
}

prepare_inno() {
    local release_root=$1 tools wine_archive compiler
    tools="$release_root/tools"
    wine_archive="wine-stable-$wine_release-osx64.tar.xz"
    mkdir -p "$tools"
    export WINEPREFIX="$tools/inno-$inno_version"
    export WINEDEBUG=-all WINEDLLOVERRIDES='winemenubuilder.exe=d;mscoree,mshtml='
    wine="${HONGGUO_WINE:-$tools/Wine Stable.app/Contents/Resources/wine/bin/wine}"
    if [[ -z ${HONGGUO_WINE:-} && ! -x $wine ]]; then
        if [[ $(uname -m) == arm64 ]]; then
            arch -x86_64 /usr/bin/true || fail "Rosetta is required by the local Inno Setup compiler"
        fi
        download_asset Gcenx/macOS_Wine_builds "$wine_release" "$tools/$wine_archive" "$wine_sha"
        tar -xJf "$tools/$wine_archive" -C "$tools"
    fi
    "$wine" --version

    compiler="$WINEPREFIX/drive_c/InnoSetup"
    local inno="$tools/innosetup-$inno_version.exe" translation="$tools/ChineseSimplified-$translation_commit.isl"
    if [[ ! -f $compiler/ISCC.exe ]]; then
        download_asset jrsoftware/issrc "is-${inno_version//./_}" "$inno" "$inno_sha"
        "$wine" "$inno" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- '/DIR=C:\InnoSetup' '/TASKS='
        [[ -f $compiler/ISCC.exe ]] || fail "Inno Setup did not install to $compiler"
    fi
    if [[ ! -f $translation ]]; then
        curl -fsSL --retry 3 "https://raw.githubusercontent.com/jrsoftware/issrc/$translation_commit/Files/Languages/ChineseSimplified.isl" -o "$translation.part"
        check_hash "$translation_sha" "$translation.part"
        mv "$translation.part" "$translation"
    fi
    check_hash "$translation_sha" "$translation"
    cp "$translation" "$compiler/Languages/ChineseSimplified.isl"
}

download_asset() {
    local asset_repo=$1 tag=$2 file=$3 digest=$4
    if [[ ! -f $file ]]; then
        gh release download "$tag" --repo "github.com/$asset_repo" --pattern "${file##*/}" --output "$file.part" --clobber
        check_hash "$digest" "$file.part"
        mv "$file.part" "$file"
    fi
    check_hash "$digest" "$file"
}

check_hash() {
    printf '%s  %s\n' "$1" "$2" | shasum -a 256 -c -
}

build_desktop() {
    local output=$1 nsh="$PWD/target/nsh-cli/debug/nsh"
    local arch triple out
    for arch in arm64 x86_64; do
        case "$arch" in
            arm64) triple=aarch64-apple-darwin ;;
            x86_64) triple=x86_64-apple-darwin ;;
        esac
        out="$output/$triple"
        "$nsh" package crates/hongguo-nsh --target macos --arch "$arch" --out "$out"
        cargo build-tool release-assets "$triple" "$out" "$output/assets"
    done
    out="$output/$windows_triple"
    build_windows "$nsh" "$out"
    cargo build-tool release-assets "$windows_triple" "$out" "$output/assets"
}

build_windows() (
    local nsh=$1 out=$2 script_path output_path
    export NSH_WINDOWS_RC
    NSH_WINDOWS_RC=$(command -v llvm-rc)
    export RC_x86_64_pc_windows_msvc="$NSH_WINDOWS_RC"
    export PATH="$PWD/target/nsh-cross-tools:$PATH"
    export XWIN_ARCH=x86_64 CARGO_INCREMENTAL=0
    export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-cpu=x86-64 -C target-feature=+crt-static -C link-arg=/STACK:8388608'
    "$nsh" package crates/hongguo-nsh --target windows --out "$out"
    script_path=$("$wine" winepath.exe -w "$out/guava.iss" | tr -d '\r')
    output_path=$("$wine" winepath.exe -w "$out" | tr -d '\r')
    "$wine" "$WINEPREFIX/drive_c/InnoSetup/ISCC.exe" /Q "/O$output_path" "$script_path"
)

push_release_tag() {
    local tag=$1 commit=$2 old_remote=$3
    local ref="refs/tags/$tag" current_remote
    current_remote=$(tag_commit "$ref")
    [[ $current_remote == "$old_remote" || $current_remote == "$commit" ]] ||
        fail "Remote $tag changed during the build"
    if [[ -z $current_remote ]]; then
        if ! git show-ref --verify --quiet "$ref"; then
            git tag -a "$tag" "$commit" -m "番果 $tag"
        fi
        cargo build-tool release-preflight --tag "$tag"
        git push --no-follow-tags origin "$ref:$ref"
    fi
    [[ $(tag_commit "$ref") == "$commit" ]] || fail "Remote $tag does not point to $commit"
}

publish_release() {
    local tag=$1 draft=$2 marker=$3 output=$4 current_draft
    current_draft=$(release_draft "$tag")
    [[ $current_draft == "$draft" ]] || fail "Release $tag changed during the build. Run the same command again."
    printf '\n%s\n' "$marker" >>"$output/release-notes.md"
    if [[ -z $draft ]]; then
        gh release create "$tag" --repo "$github_repository" --verify-tag --draft --title "番果 $tag" --notes-file "$output/release-notes.md"
    fi
    gh release upload "$tag" "$output"/assets/* --repo "$github_repository" --clobber
    gh release edit "$tag" --repo "$github_repository" --draft=false --latest
}

main "$@"
