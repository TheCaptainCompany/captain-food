#!/usr/bin/env python3
"""Local JWKS stub + RS256 token minting for the walk.

WHAT THIS IS, AND WHAT IT DELIBERATELY IS NOT
---------------------------------------------
This stubs **one endpoint**: the JWKS document. Nothing else about authentication is faked.

The verifier (`crates/server/src/auth.rs`) is used **unmodified and fail-closed**. It still
requires, for every role path:

  1. a signature it can verify against a key in the JWKS at `SUPABASE_JWKS_URL`;
  2. `iss` exactly equal to `{SUPABASE_URL}/auth/v1`;
  3. `aud` == "authenticated";
  4. `app_metadata.captain_food` carrying a role it recognises;
  5. that role EQUAL to the path role (strict equality -- an ADMIN token cannot walk /restaurant).

`SUPABASE_URL` therefore keeps its real, baked production value, so the issuer string the walk
signs is the real issuer string. Only the key-distribution endpoint moves to loopback, because
Supabase is unreachable from this environment (proxy answers 502 to CONNECT, a policy denial).

This is NOT a GoTrue fake. It cannot register a user, cannot send or verify an OTP, and cannot
stamp `app_metadata` on a real Supabase user. That is on purpose: a fake that minted convincing
customers would make the walk's registration leg *look* real while proving nothing. The
registration leg is left RED and named instead.
"""

import argparse
import base64
import json
import os
import subprocess
import sys
import time

# The `openssl` CLI does the RSA work, NOT python-cryptography: this image ships a
# `cryptography` whose Rust bindings are broken (`ModuleNotFoundError: _cffi_backend`, which
# surfaces as a pyo3 PanicException on `import`), and `pyjwt` fails the same way. openssl 3.x is
# present and needs no install on a disk with ~4 GB free.
KID = "captain-food-walk-key-1"


def b64u(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")


def openssl(args: list, stdin: bytes = b"") -> bytes:
    proc = subprocess.run(
        ["openssl"] + args, input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if proc.returncode != 0:
        raise SystemExit(f"openssl {' '.join(args)} failed: {proc.stderr.decode()[:400]}")
    return proc.stdout


def load_or_create_key(key_path: str) -> str:
    if not os.path.exists(key_path):
        os.makedirs(os.path.dirname(key_path), exist_ok=True)
        openssl(["genrsa", "-out", key_path, "2048"])
        os.chmod(key_path, 0o600)
    return key_path


def jwks_document(key_path: str) -> dict:
    # `-noout -modulus` prints `Modulus=<uppercase hex>`. The public exponent is openssl's
    # default F4 (65537) -> 0x010001 -> "AQAB", the same value every real provider emits.
    line = openssl(["rsa", "-in", key_path, "-noout", "-modulus"]).decode().strip()
    modulus_hex = line.split("=", 1)[1]
    n = bytes.fromhex(modulus_hex)
    e = (65537).to_bytes(3, "big")
    # `alg` is present because the verifier prefers the JWK's algorithm over the header's
    # (auth.rs::asymmetric_alg) -- which is what defeats alg-confusion. Emitting it keeps the
    # stub on the same side of that defence as a real provider.
    return {
        "keys": [
            {
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": KID,
                "n": b64u(n),
                "e": b64u(e),
            }
        ]
    }


def mint(key_path: str, issuer: str, sub: str, claims: dict, ttl: int) -> str:
    header = {"alg": "RS256", "typ": "JWT", "kid": KID}
    now = int(time.time())
    # `captain_food` is nested under `app_metadata` because Supabase merges `app_metadata`
    # SHALLOWLY -- one object per product, so a sibling product's write cannot reach into ours.
    payload = {
        "sub": sub,
        "aud": "authenticated",
        "iss": f"{issuer}/auth/v1",
        "iat": now,
        "exp": now + ttl,
        "role": "authenticated",
        "app_metadata": {"provider": "phone", "captain_food": claims},
    }
    signing_input = (
        b64u(json.dumps(header, separators=(",", ":")).encode())
        + "."
        + b64u(json.dumps(payload, separators=(",", ":")).encode())
    ).encode("ascii")
    signature = openssl(["dgst", "-sha256", "-sign", key_path, "-binary"], stdin=signing_input)
    return signing_input.decode("ascii") + "." + b64u(signature)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("command", choices=["init", "mint", "decode"])
    ap.add_argument("--dir", required=True, help="stub state directory (key + served tree)")
    ap.add_argument("--issuer", default="", help="SUPABASE_URL; iss becomes {issuer}/auth/v1")
    ap.add_argument("--sub", default="", help="the token subject (the Supabase user id)")
    ap.add_argument(
        "--claims",
        default="{}",
        help="JSON object placed verbatim at app_metadata.captain_food "
        "(role, customer_id, restaurant_id, rider_id, restaurant_account_id)",
    )
    ap.add_argument("--ttl", type=int, default=3600)
    ap.add_argument("--token", default="", help="for `decode`")
    args = ap.parse_args()

    if args.command == "decode":
        body = args.token.split(".")[1]
        body += "=" * (-len(body) % 4)
        print(json.dumps(json.loads(base64.urlsafe_b64decode(body)), indent=None))
        return 0

    key_path = load_or_create_key(os.path.join(args.dir, "walk-signing-key.pem"))

    if args.command == "init":
        # The served tree mirrors the real path so SUPABASE_JWKS_URL differs from production in
        # host and scheme ONLY -- the same shape the k3s/MKS targets will use.
        served = os.path.join(args.dir, "www", "auth", "v1", ".well-known")
        os.makedirs(served, exist_ok=True)
        with open(os.path.join(served, "jwks.json"), "w") as fh:
            json.dump(jwks_document(key_path), fh)
        print(os.path.join(args.dir, "www"))
        return 0

    if args.command == "mint":
        if not args.issuer or not args.sub:
            print("mint requires --issuer and --sub", file=sys.stderr)
            return 2
        print(mint(key_path, args.issuer, args.sub, json.loads(args.claims), args.ttl))
        return 0

    return 2


if __name__ == "__main__":
    sys.exit(main())
