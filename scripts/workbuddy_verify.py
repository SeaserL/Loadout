# SPDX-License-Identifier: Apache-2.0
"""Opt-in live verification with isolated daemon credentials, or an installed route."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.request
import urllib.error
import uuid

from workbuddy_bridge_key import ROOT, protect


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--loadout", required=True)
    parser.add_argument("--production", action="store_true")
    parser.add_argument("--model", default="workbuddy/main/deepseek-v4.1-flash",
                        choices=["workbuddy/main/deepseek-v4.1-flash", "loadout/workbuddy-flash"])
    args = parser.parse_args()
    if os.environ.get("LOADOUT_LIVE_TESTS") != "1":
        raise RuntimeError("LOADOUT_LIVE_TESTS=1 required")
    unique = uuid.uuid4().hex
    prefix = [str(Path(args.loadout).resolve()), "--json"]
    if not args.production:
        prefix += ["--pipe-name", "\\\\.\\pipe\\loadout-workbuddy-test-" + unique]

    def cli(*words, input_text=None):
        result = subprocess.run(prefix + list(words), input=input_text, text=True,
                                capture_output=True, timeout=60, encoding="utf-8")
        if result.returncode:
            raise RuntimeError("CLI failed: " + " ".join(words[:2]))
        return json.loads(result.stdout) if result.stdout.strip() else None

    client_name = "workbuddy-verify-" + unique[:8]
    started = False
    account_added = False
    client_added = False
    temporary = tempfile.TemporaryDirectory(prefix="loadout-workbuddy-test-")
    try:
        if not args.production:
            cli("daemon", "start", "--data-dir", temporary.name, "--port", "0",
                "--instance-id", unique, "--secret-prefix", "loadout-e2e-" + unique + ":")
            started = True
            cli("provider", "add", "workbuddy", "--kind", "workbuddy-bridge")
            key = protect((ROOT / "bridge-key.dpapi").read_bytes(), True).decode()
            cli("account", "add", "workbuddy", "main", "--api-key-stdin", input_text=key + "\n")
            account_added = True
            before = cli("route", "status")
            if "workbuddy/main/deepseek-v4.1-flash" in json.dumps(before):
                raise RuntimeError("Route appeared before opt-in")
            cli("account", "transport", "workbuddy", "main", "on", "--i-understand-the-risk", "--confirm", "main")
        status = cli("daemon", "status")
        address = status["gateway"]["addr"]
        created = cli("client", "create", client_name)
        client_added = True
        key = created["key"]
        exact = "workbuddy/main/deepseek-v4.1-flash"
        cli("client", "policy", client_name, "--allow", exact, "--allow", args.model)
        for streaming in [False, True]:
            body = {"model": args.model, "messages": [{"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Reply with exactly LOADOUT_OK."}],
                    "max_tokens": 256, "stream": streaming}
            request = urllib.request.Request("http://" + address + "/v1/chat/completions",
                data=json.dumps(body).encode(), headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
            try:
                response = urllib.request.urlopen(request, timeout=60)
            except urllib.error.HTTPError as error:
                payload = json.load(error)
                detail = payload.get("error", {})
                print(json.dumps({"http_status": error.code, "error_type": detail.get("type"), "error_code": detail.get("code"), "message": str(detail.get("message", ""))[:200].replace(key, "[REDACTED]")}))
                raise RuntimeError("Gateway smoke failed") from None
            with response:
                raw = response.read().decode()
                if streaming:
                    content = ""
                    for line in raw.splitlines():
                        if line.startswith("data:") and line[5:].strip() != "[DONE]":
                            event = json.loads(line[5:])
                            for choice in event.get("choices", []):
                                content += choice.get("delta", {}).get("content", "")
                    if "LOADOUT_OK" not in content or "[DONE]" not in raw:
                        raise RuntimeError("Stream marker or DONE missing")
                else:
                    payload = json.loads(raw)
                    if "LOADOUT_OK" not in payload["choices"][0]["message"]["content"]:
                        raise RuntimeError("Response marker missing")
                print(json.dumps({"scope": "production" if args.production else "isolated", "stream": streaming,
                                  "http_status": response.status, "marker_verified": True, "requested_model": args.model}), flush=True)
        routes = cli("route", "status")
        print(json.dumps({"route_present": exact in json.dumps(routes), "scope": "production" if args.production else "isolated"}))
    finally:
        if client_added:
            cli("client", "revoke", client_name)
        if account_added:
            cli("account", "remove", "workbuddy", "main")
        if started:
            cli("daemon", "stop")
        temporary.cleanup()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(json.dumps({"status": "FAILED", "error_type": type(error).__name__,
                          "message": str(error) if type(error) is RuntimeError else "Verification failed; no secret output"}))
        raise SystemExit(1)
