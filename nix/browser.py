# A browser's side of phoebe's sessions, for the VM test. `browser signin
# <url>` makes a P-256 key pair, signs in at a node's sign-in address with
# it, and keeps the key and the session in the file $PHOEBE_SESSION names;
# it exits 1 and prints the refusal when the node refuses. `browser curl
# <path> [curl args]` asks the app's socket for <path>, signed with that
# session as the web app signs every request.
import base64
import hashlib
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.request

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature

ORIGIN = os.environ.get("PHOEBE_ORIGIN", "https://phoebe.test")
SESSION = os.environ.get("PHOEBE_SESSION", "/root/phoebe-session.json")
SOCKET = os.environ.get("PHOEBE_SOCKET", "/run/phoebe-web/origin.sock")


def b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def sign(key, message):
    """r || s over the message, as WebCrypto signs"""
    r, s = decode_dss_signature(key.sign(message.encode(), ec.ECDSA(hashes.SHA256())))
    return b64(r.to_bytes(32, "big") + s.to_bytes(32, "big"))


def now():
    return int(time.time() * 1000)


def signin(url):
    key = ec.generate_private_key(ec.SECP256R1())
    numbers = key.public_key().public_numbers()
    ts = now()
    body = {
        "key": {
            "kty": "EC",
            "crv": "P-256",
            "x": b64(numbers.x.to_bytes(32, "big")),
            "y": b64(numbers.y.to_bytes(32, "big")),
        },
        "ts": ts,
        "sig": sign(key, f"phoebe-signin-v1\n{ORIGIN}\n{ts}"),
    }
    request = urllib.request.Request(
        f"{url}/auth/session",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json", "origin": ORIGIN},
    )
    # the node's certificate is the test's own, which nothing here trusts
    context = ssl._create_unverified_context()
    try:
        with urllib.request.urlopen(request, context=context) as response:
            answer = json.load(response)
    except urllib.error.HTTPError as error:
        print(f"{error.code} {error.read().decode()}")
        sys.exit(1)
    pem = key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    ).decode()
    with open(SESSION, "w") as file:
        json.dump({"key": pem, "token": answer["token"]}, file)
    print(json.dumps(answer["viewer"]))


def curl(path, args):
    with open(SESSION) as file:
        session = json.load(file)
    key = serialization.load_pem_private_key(session["key"].encode(), None)
    token = session["token"]
    ts = now()
    digest = b64(hashlib.sha256(token.encode()).digest())
    proof = sign(key, f"phoebe-req-v1\nGET\n{path}\n{ts}\n{digest}")
    os.execvp(
        "curl",
        [
            "curl",
            "-s",
            "--unix-socket",
            SOCKET,
            "-H",
            f"authorization: Phoebe {token}",
            "-H",
            f"phoebe-proof: {ts}.{proof}",
            *args,
            f"http://phoebe{path}",
        ],
    )


if __name__ == "__main__":
    match sys.argv[1:]:
        case ["signin", url]:
            signin(url)
        case ["curl", path, *args]:
            curl(path, args)
        case _:
            sys.exit("usage: browser signin <url> | browser curl <path> [curl args]")
