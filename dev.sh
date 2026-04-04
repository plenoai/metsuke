#!/usr/bin/env bash
set -euo pipefail

# Metsuke local development script
# Uses vercel-labs/emulate for GitHub API mock

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

EMULATE_PID=""
METSUKE_PID=""

cleanup() {
  echo "Shutting down..."
  [ -n "$EMULATE_PID" ] && kill "$EMULATE_PID" 2>/dev/null || true
  [ -n "$METSUKE_PID" ] && kill "$METSUKE_PID" 2>/dev/null || true
  wait 2>/dev/null
}
trap cleanup EXIT

# 1. Start emulate (GitHub mock on port 4001)
echo "Starting GitHub API emulator on :4001..."
npx emulate --service github --port 4001 &
EMULATE_PID=$!
sleep 2

# 2. Start metsuke with dev config
export GITHUB_APP_ID=12345
export GITHUB_APP_CLIENT_ID=Iv1.dev0000000000
export GITHUB_APP_CLIENT_SECRET=dev-secret
export GITHUB_APP_PRIVATE_KEY="-----BEGIN RSA PRIVATE KEY-----
MIIEogIBAAKCAQEAqzbtQHmqIhOu650zIypzursOHTZhGltXZQ/aeDV8/XEADzQJ
vQ0wwhs1gae9/31tfQk/osJ93CH3QaBTS7VPHBGvd057hDbTq+Y7QAsVTy1SNIEN
Syh8CI+3oCXMkaZDpDQvFMBKjOYnHunqEQTIE6YRJX3dceFKDnEQ0nU9MLbwCRDW
QFFubzLIIP+BkzF1DC8jwwmU+nHtj+UD7izo6t8tsOeZmT6QjjAufYdsejbww0Ev
sN+mnoag2zHBNBpG6m/fIw9Hcvflo73T7i9iuPPoBpwKdx8COrjo/AxaghatJ7rJ
g3hdMPVBcIVUgmfWNYF11wsnTyLFmP0PWDUw1wIDAQABAoIBABxu2TSZX88b7LMV
Ho5q+OAcO0pPow2Q+LEAUnwfCdw+3U8pCar7G0tI4HhhJnTc3AdlN0ust+EMRPcB
jIOonvQe3cBW6L06q6lC6TkH/ihxctLkUZRXK03yrABs9o2Din0k62KrUlYWzI1e
NDBSVnWo4PUUc2d7jeRbE3uX26sQmHL7cSsNq8gyZQO6snR1t0mgnV8x66l+tsYy
Jt2rKg8bN1yVMUMeXIC+tf2/uhnIPdkAlu7S1/vMhqRfeJK+/56vuW+f5ilH1/12
OQx+jaUbVxuVkgFsiAYkLf202TGJFkK97UazFtrni+2Qp1YrZyiYFIiy/hGK58MI
FEGeNSUCgYEA5cLKtdQyKQ8rCXyTYCatJYC3xo/ZKnDxw4bVZR/cnsE9ueLCA9of
PW/AoDVj06XkDpI1EooP83BKlL02Uzf+YqALYGDGTQ9M+VBBnFNIrkKiYGFIQd99
RiHFPv2z/ix7ixs2vWHy7M6RPkSPzqR491OaIjk39ikvEANpDEy0kOMCgYEAvsR+
pWc4vL1lZFbIlzGvR4EGXXmVR/xKR/URtlsCQyuWmQiG2fbgw/ZigeMAcncNNXQD
9w9dhIWMI/6B27WHrAikEmK1OwvxXV1rDsohcozaTenIjj8bP/T1ZstXgSYY1qpX
zWgvHM+19GW6D9+uCWXzG6nJAzKPcLa8Z2VwZn0CgYAMRwJqAPLFOuhD04JUivyJ
mn03gQxLtklU92mDw9YYLZ9MxY80gX1V3Rjf9rpk3uJ23N01Jmd/zKpPlGTIwZ84
SfERr1opV/32/JDk95ZUqX7fw5MG4hhhnQBbQ1dQ57OaVVPxfsBqYwdj2moM0sEc
Bj2gQop4/u5i3qvIWnjznQKBgG77Q66YaZKsIMOKFXKYbh+MOZbB+A4EAXbxZReQ
xLUtM5TeOA2wKbz3pwFnfcgZ6K5TS0c9QiupwgjitMuMRVzZPhKQKF0sqoOlqHXX
NDQ/K3Wub4YJwqGnsejWnZa+Ai9ItIIEfXwmfvWrBN7dQ5OmIxPR5+abUIXDWcJR
al3FAoGAehWD1fvBiaKjCWdmV8/+ibed+kMGp5GjfBLS+QwD/dRpH/Xk0duuN8a2
DyPFdryNIZr/lOYzmWgXIpbTSufDiaac8ijgVKZZamEGWNrHZ5eu++9tWI9PEPtQ
J9/Y+qX1+dFvHem00HtuVTs2mItUXlLIOAlgtrWHl0pIzYSARxM=
-----END RSA PRIVATE KEY-----"
export DATABASE_URL="$SCRIPT_DIR/dev.db"
export BASE_URL="http://localhost:8080"
export GITHUB_API_HOST="localhost:4001"
export GITHUB_WEB_HOST="localhost:4001"
export HOST="127.0.0.1"
export PORT="8080"

echo "Starting metsuke on http://localhost:8080..."
cargo run -p metsuke &
METSUKE_PID=$!

echo ""
echo "=== Metsuke Dev Server ==="
echo "  App:    http://localhost:8080"
echo "  GitHub: http://localhost:4001 (emulated)"
echo ""
echo "Press Ctrl+C to stop."
wait
