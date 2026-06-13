#!/usr/bin/env bash
# Build a .rpm of noip-duc from the local repo.
#
# Usage:
#   dist/rpm/build.sh                # build SRPM + binary RPM into ./dist/rpm/out
#   dist/rpm/build.sh --srpm-only    # just the source RPM
#   dist/rpm/build.sh --mock         # build via mock(1) for a clean chroot build
#
# Outputs:
#   dist/rpm/out/SRPMS/noip-duc-<ver>-<rel>.src.rpm
#   dist/rpm/out/RPMS/<arch>/noip-duc-<ver>-<rel>.<arch>.rpm

set -euo pipefail

# Resolve repo root regardless of CWD.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
SPEC="${SCRIPT_DIR}/noip-duc.spec"

SRPM_ONLY=0
USE_MOCK=0
for arg in "$@"; do
    case "$arg" in
        --srpm-only) SRPM_ONLY=1 ;;
        --mock)      USE_MOCK=1 ;;
        -h|--help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *)
            echo "unknown arg: $arg" >&2
            exit 2
            ;;
    esac
done

# Read Name + Version from the spec so we don't drift.
NAME=$(awk '/^Name:[[:space:]]+/   { print $2; exit }' "$SPEC")
VERSION=$(awk '/^Version:[[:space:]]+/{ print $2; exit }' "$SPEC")
TARBALL="${NAME}-${VERSION}.tar.gz"

OUT="${SCRIPT_DIR}/out"
RPMTREE="${OUT}/rpmtree"
rm -rf "${RPMTREE}"
mkdir -p "${RPMTREE}"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}

echo ":: building source tarball -> ${RPMTREE}/SOURCES/${TARBALL}"
# Tar the working tree minus build artefacts. `git ls-files` would be
# cleaner but this script must work in a freshly extracted source tree
# too, so we use a tar exclude list.
( cd "${REPO_ROOT}" && \
  tar --exclude=./target \
      --exclude=./dist/rpm/out \
      --exclude=./.git \
      --exclude=./.github \
      --exclude='./*.rpm' \
      --transform "s,^\.,${NAME}-${VERSION}," \
      -czf "${RPMTREE}/SOURCES/${TARBALL}" \
      . )

cp "${SPEC}" "${RPMTREE}/SPECS/"

if [[ "${USE_MOCK}" -eq 1 ]]; then
    if ! command -v mock >/dev/null 2>&1; then
        echo "error: mock not installed (sudo dnf install mock)" >&2
        exit 1
    fi
    echo ":: building SRPM"
    rpmbuild --define "_topdir ${RPMTREE}" -bs "${RPMTREE}/SPECS/$(basename "${SPEC}")"

    SRPM=$(ls "${RPMTREE}/SRPMS/"*.src.rpm | head -n1)
    echo ":: building binary RPM via mock from ${SRPM}"
    mock --resultdir="${OUT}/RPMS" --rebuild "${SRPM}"
    cp "${SRPM}" "${OUT}/SRPMS/" || mkdir -p "${OUT}/SRPMS" && cp "${SRPM}" "${OUT}/SRPMS/"
elif [[ "${SRPM_ONLY}" -eq 1 ]]; then
    echo ":: building SRPM only"
    rpmbuild --define "_topdir ${RPMTREE}" -bs "${RPMTREE}/SPECS/$(basename "${SPEC}")"
    mkdir -p "${OUT}/SRPMS"
    cp "${RPMTREE}/SRPMS/"*.src.rpm "${OUT}/SRPMS/"
else
    echo ":: building binary + source RPM (host build)"
    rpmbuild --define "_topdir ${RPMTREE}" -ba "${RPMTREE}/SPECS/$(basename "${SPEC}")"
    mkdir -p "${OUT}/RPMS" "${OUT}/SRPMS"
    cp -r "${RPMTREE}/RPMS/"* "${OUT}/RPMS/"
    cp    "${RPMTREE}/SRPMS/"*.src.rpm "${OUT}/SRPMS/"
fi

echo
echo ":: artefacts under ${OUT}"
find "${OUT}" -name '*.rpm' -printf '   %p\n'
