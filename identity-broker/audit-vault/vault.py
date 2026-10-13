"""Independent audit ingestion function. No archive decryption keys or delete API.

Pin one stream and Ed25519 public key in the security team's deployment.
Cloud objects are held indefinitely; disposition is a separate security-team process.
"""
import base64
import hashlib
import hmac
import json
import os
import re
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

MAX_PACKET = 6 * 1024 * 1024


def require(value, message):
    if not value:
        raise ValueError(message)


def decode(value):
    require(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]+", value), "Invalid base64url")
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def digest(payload):
    return base64.urlsafe_b64encode(hashlib.sha256(payload.encode()).digest()).decode().rstrip("=")


def parse(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=unique)


def signed_value(packet, key):
    require(isinstance(packet, dict) and set(packet) == {"payload", "signature"}, "Invalid signed envelope")
    require(isinstance(packet["payload"], str), "Invalid payload")
    key.verify(decode(packet["signature"]), packet["payload"].encode())
    return parse(packet["payload"])


class Vault:
    def __init__(self, sink, stream, public_key):
        require(re.fullmatch(r"[A-Za-z0-9_-]{1,128}", stream), "Invalid stream")
        self.sink, self.stream, self.public = sink, stream, public_key
        self.key = Ed25519PublicKey.from_public_bytes(decode(public_key))

    def validate(self, packet):
        batch = signed_value(packet, self.key)
        require(batch["schema"] == 1 and batch["stream"] == self.stream and batch["public_key"] == self.public, "Wrong audit identity")
        first, last = batch["first"], batch["last"]
        require(type(first) is int and type(last) is int and 1 <= first <= last and last-first < 64, "Invalid sequence range")
        require(isinstance(batch["events"], list) and len(batch["events"]) == last-first+1, "Missing events")
        previous = batch["previous_event"]
        for seq, event in enumerate(batch["events"], first):
            value = signed_value(event, self.key)
            require(value["schema"] == 1 and value["stream"] == self.stream and value["seq"] == seq and value["previous"] == previous, "Event chain mismatch")
            previous = digest(event["payload"])
        require(previous == batch["last_event"], "Wrong terminal event hash")
        return batch

    def receive(self, raw):
        require(len(raw) <= MAX_PACKET, "Packet too large")
        packet = parse(raw)
        batch = self.validate(packet)
        batch_id = digest(packet["payload"])
        prefix = self.stream + "/"
        if batch["first"] == 1:
            require(batch["previous_batch"] == batch["previous_event"] == "", "Invalid genesis")
        else:
            require(re.fullmatch(r"[A-Za-z0-9_-]{43}", batch["previous_batch"]), "Invalid predecessor hash")
            predecessor = parse(self.sink.get(prefix + "hash/" + batch["previous_batch"] + ".json"))
            prior = self.validate(predecessor)
            require(digest(predecessor["payload"]) == batch["previous_batch"] and prior["last"]+1 == batch["first"] and prior["last_event"] == batch["previous_event"], "Missing or conflicting predecessor")
        encoded = json.dumps(packet, separators=(",", ":"), ensure_ascii=False).encode()
        # First sequence is a create-only concurrency guard. A different batch cannot replace it.
        # Hash copy allows bounded predecessor lookup. Retry repairs a crash between these writes.
        self.sink.put_once(prefix + f"first/{batch['first']:020d}.json", encoded)
        self.sink.put_once(prefix + "hash/" + batch_id + ".json", encoded)
        return {"stored_sha256": batch_id, "immutable": True}


class S3Sink:
    def __init__(self, bucket):
        import boto3
        self.client, self.bucket = boto3.client("s3"), bucket

    def get(self, name):
        response = self.client.get_object(Bucket=self.bucket, Key=name)
        require(response.get("ObjectLockLegalHoldStatus") == "ON", "Object is not held")
        data = response["Body"].read(MAX_PACKET+1)
        require(len(data) <= MAX_PACKET, "Stored packet too large")
        return data

    def put_once(self, name, data):
        from botocore.exceptions import ClientError
        try:
            self.client.put_object(Bucket=self.bucket, Key=name, Body=data, ContentType="application/json",
                                   IfNoneMatch="*", ObjectLockLegalHoldStatus="ON")
        except ClientError as error:
            if error.response["Error"]["Code"] not in ("PreconditionFailed", "412"):
                raise
        require(self.get(name) == data, "Conflicting immutable object")


class AzureSink:
    def __init__(self, account_url, container):
        from azure.identity import DefaultAzureCredential
        from azure.storage.blob import BlobServiceClient
        require(re.fullmatch(r"https://[a-z0-9]{3,24}\.blob\.core\.windows\.net", account_url), "Invalid Azure storage endpoint")
        self.container = BlobServiceClient(account_url, credential=DefaultAzureCredential()).get_container_client(container)

    def check_hold(self):
        require(self.container.get_container_properties().has_legal_hold, "Container must have a security-admin-managed legal hold")

    def get(self, name):
        self.check_hold()
        blob = self.container.get_blob_client(name)
        require(blob.get_blob_properties().size <= MAX_PACKET, "Stored packet too large")
        return blob.download_blob().readall()

    def put_once(self, name, data):
        from azure.core.exceptions import AzureError, ResourceNotFoundError
        self.check_hold()
        try:
            require(self.get(name) == data, "Conflicting immutable object")
            return
        except ResourceNotFoundError:
            pass
        try:
            self.container.upload_blob(name, data, overwrite=False)
        except AzureError:
            # Concurrent create/retry may produce different 409/412 codes; only
            # acknowledge if a held object with exactly these bytes now exists.
            pass
        require(self.get(name) == data, "Conflicting immutable object")


class GcpSink:
    def __init__(self, bucket):
        from google.cloud import storage
        self.bucket = storage.Client().bucket(bucket)

    def get(self, name):
        blob = self.bucket.get_blob(name)
        require(blob is not None and blob.temporary_hold and blob.size <= MAX_PACKET, "Missing, unheld or oversized object")
        return blob.download_as_bytes(if_generation_match=blob.generation)

    def put_once(self, name, data):
        from google.api_core.exceptions import PreconditionFailed
        blob = self.bucket.blob(name)
        blob.temporary_hold = True
        try:
            blob.upload_from_string(data, content_type="application/json", if_generation_match=0)
        except PreconditionFailed:
            pass
        require(self.get(name) == data, "Conflicting immutable object")


_vault = None


def configured_vault():
    global _vault
    if _vault is None:
        provider = os.environ["LTTL_VAULT_PROVIDER"]
        if provider == "aws":
            sink = S3Sink(os.environ["LTTL_VAULT_BUCKET"])
        elif provider == "azure":
            sink = AzureSink(os.environ["LTTL_VAULT_ACCOUNT_URL"], os.environ["LTTL_VAULT_CONTAINER"])
        elif provider == "gcp":
            sink = GcpSink(os.environ["LTTL_VAULT_BUCKET"])
        else:
            raise ValueError("Unsupported vault provider")
        _vault = Vault(sink, os.environ["LTTL_VAULT_STREAM"], os.environ["LTTL_VAULT_PUBLIC_KEY"])
    return _vault


def handle(method, headers, body):
    headers = {k.lower(): v for k, v in headers.items()}
    secret = os.environ.get("LTTL_VAULT_INGEST_TOKEN", "")
    if len(secret) < 32 or not hmac.compare_digest(headers.get("authorization", ""), "Bearer " + secret):
        return 403, {"error": "Access denied"}
    if method != "POST":
        return 405, {"error": "POST required"}
    try:
        return 200, configured_vault().receive(body)
    except Exception:
        # Do not expose provider responses, credentials or audit contents in error bodies/logs.
        return 409, {"error": "Audit batch not acknowledged; inspect vault configuration and continuity"}


def lambda_handler(event, context):
    body = event.get("body") or ""
    body = base64.b64decode(body) if event.get("isBase64Encoded") else body.encode()
    method = event.get("requestContext", {}).get("http", {}).get("method", event.get("httpMethod", ""))
    status, result = handle(method, event.get("headers") or {}, body)
    return {"statusCode": status, "headers": {"content-type": "application/json", "cache-control": "no-store"}, "body": json.dumps(result)}


def gcp_handler(request):
    status, result = handle(request.method, dict(request.headers), request.stream.read(MAX_PACKET+1))
    return json.dumps(result), status, {"Content-Type": "application/json", "Cache-Control": "no-store"}


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Offline verification of an exported signed audit batch")
    parser.add_argument("packet")
    parser.add_argument("--public-key", required=True)
    parser.add_argument("--stream", required=True)
    args = parser.parse_args()
    from pathlib import Path
    raw = Path(args.packet).read_bytes()
    require(len(raw) <= MAX_PACKET, "Packet too large")
    batch = Vault(None, args.stream, args.public_key).validate(parse(raw))
    print(json.dumps({"verified": True, "first": batch["first"], "last": batch["last"], "note": "Verify predecessor batches for complete continuity"}))
