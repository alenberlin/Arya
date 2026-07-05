#!/usr/bin/env bash
#
# Creates a stable, self-signed code-signing identity ("Arya Local Signing") in
# the login keychain, so local builds sign with a persistent certificate instead
# of an ad-hoc signature.
#
# Why this matters: macOS TCC (Input Monitoring, Accessibility, Microphone) keys
# a granted permission to the app's *designated requirement*. An ad-hoc signature
# pins that to the binary's cdhash, which changes on every rebuild — so every
# rebuild silently revokes the grant and the user must re-permit. A certificate
# signature pins it to `certificate leaf = H"…"` (the cert), which is stable
# across rebuilds, so the grant is remembered until the cert changes.
#
# This is NOT notarization: the cert is untrusted by Gatekeeper, so a fresh
# download still needs a one-time right-click → Open. It only stabilizes TCC.
# A paid Developer ID + notarization supersedes this entirely.
#
# Idempotent: re-running does nothing if the identity already exists.
#
# Usage: scripts/create-signing-cert.sh
set -euo pipefail

CN="Arya Local Signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
CERT_DIR="$HOME/.arya-signing"

if security find-certificate -c "$CN" >/dev/null 2>&1; then
  echo "==> '$CN' already exists in the keychain — nothing to do."
  echo "    Packaging will use it automatically."
  exit 0
fi

echo "==> Generating a self-signed code-signing certificate ('$CN')…"
mkdir -p "$CERT_DIR"
cat > "$CERT_DIR/openssl.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = v3
prompt = no
[dn]
CN = $CN
[v3]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

openssl req -x509 -newkey rsa:2048 -keyout "$CERT_DIR/key.pem" -out "$CERT_DIR/cert.pem" \
  -days 3650 -nodes -config "$CERT_DIR/openssl.cnf" >/dev/null 2>&1

# macOS `security` reads only the legacy SHA1-MAC PKCS#12 form; OpenSSL 3 defaults
# to a SHA256 MAC it rejects with "MAC verification failed".
openssl pkcs12 -export -inkey "$CERT_DIR/key.pem" -in "$CERT_DIR/cert.pem" \
  -out "$CERT_DIR/arya.p12" -passout pass:arya \
  -macalg sha1 -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -name "$CN" >/dev/null 2>&1

# -A lets codesign use the key without a per-signature keychain prompt. The cert
# stays untrusted for Gatekeeper (fine — codesign signs with it regardless).
security import "$CERT_DIR/arya.p12" -k "$KEYCHAIN" -P arya -T /usr/bin/codesign -A >/dev/null

echo "==> Done. Local builds will now sign with '$CN'."
echo "    Grant Input Monitoring / Accessibility once more after the next build;"
echo "    subsequent rebuilds keep the grant (stable certificate identity)."
