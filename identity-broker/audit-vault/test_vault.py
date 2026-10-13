import copy
import json
import threading
import unittest
from unittest.mock import patch
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
import base64
import vault

def encode(raw):
    return base64.urlsafe_b64encode(raw).decode().rstrip("=")

class MemorySink:
    """Test double only. Never advertises filesystem/memory storage as production WORM."""
    def __init__(self):
        self.objects, self.lock, self.fail = {}, threading.Lock(), False
    def get(self, name):
        return self.objects[name]
    def put_once(self, name, data):
        if self.fail:
            raise OSError("simulated storage outage")
        with self.lock:
            if name in self.objects and self.objects[name] != data:
                raise ValueError("conflict")
            self.objects[name] = data

class Tests(unittest.TestCase):
    def setUp(self):
        self.key = Ed25519PrivateKey.generate()
        self.public = encode(self.key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw))
        self.sink = MemorySink()
        self.vault = vault.Vault(self.sink, "test", self.public)
    def sign(self, value):
        payload = json.dumps(value, separators=(",", ":"))
        return {"payload": payload, "signature": encode(self.key.sign(payload.encode()))}
    def packet(self, first=1, previous_batch="", previous_event="", kind="key_released"):
        event = self.sign({"schema":1, "stream":"test", "seq":first, "previous":previous_event, "event":kind})
        return self.sign({"schema":1,"stream":"test","public_key":self.public,"first":first,"last":first,
                          "previous_batch":previous_batch,"previous_event":previous_event,
                          "last_event":vault.digest(event["payload"]),"events":[event]})
    def receive(self, packet):
        return self.vault.receive(json.dumps(packet).encode())
    def test_signed_retry_and_predecessor(self):
        first = self.packet()
        result = self.receive(first)
        self.assertEqual(self.receive(first), result)
        second = self.packet(2,result["stored_sha256"],json.loads(first["payload"])["last_event"])
        self.receive(second)
        self.assertEqual(len(self.sink.objects),4)
    def test_conflicting_signed_history_is_rejected(self):
        self.receive(self.packet())
        with self.assertRaises(ValueError): self.receive(self.packet(kind="key_denied"))
    def test_modified_payload_and_wrong_key(self):
        packet = self.packet(); packet["payload"] += " "
        with self.assertRaises(Exception): self.receive(packet)
        packet = self.packet()
        packet["signature"] = encode(Ed25519PrivateKey.generate().sign(packet["payload"].encode()))
        with self.assertRaises(Exception): self.receive(packet)
    def test_missing_predecessor_and_missing_event(self):
        with self.assertRaises(Exception): self.receive(self.packet(2,"a"*43,"b"*43))
        value=json.loads(self.packet()["payload"]);value["events"]=[]
        with self.assertRaises(ValueError): self.receive(self.sign(value))
    def test_invalid_event_signature_not_rescued_by_batch_signature(self):
        value=json.loads(self.packet()["payload"]); value["events"][0]["signature"]="AA"
        with self.assertRaises(Exception): self.receive(self.sign(value))
    def test_outage_leaves_no_ack_and_retry_repairs_partial_write(self):
        packet=self.packet();self.sink.fail=True
        with self.assertRaises(OSError):self.receive(packet)
        self.sink.fail=False;self.receive(packet)
        del self.sink.objects["test/hash/"+vault.digest(packet["payload"])+".json"]
        self.receive(packet);self.assertEqual(len(self.sink.objects),2)
    def test_concurrent_identical_delivery(self):
        packet=self.packet();errors=[]
        def run():
            try:self.receive(packet)
            except Exception as e:errors.append(e)
        threads=[threading.Thread(target=run) for _ in range(8)]
        for t in threads:t.start()
        for t in threads:t.join()
        self.assertEqual(errors,[]);self.assertEqual(len(self.sink.objects),2)
    def test_receiver_authentication_and_oversize(self):
        with patch.dict("os.environ",{"LTTL_VAULT_INGEST_TOKEN":"x"*32}),patch.object(vault,"_vault",self.vault):
            self.assertEqual(vault.handle("POST",{},b"{}")[0],403)
            self.assertEqual(vault.handle("GET",{"Authorization":"Bearer "+"x"*32},b"")[0],405)
            self.assertEqual(vault.handle("POST",{"Authorization":"Bearer "+"x"*32},json.dumps(self.packet()).encode())[0],200)
            self.assertEqual(vault.handle("POST",{"Authorization":"Bearer "+"x"*32},b" "*(vault.MAX_PACKET+1))[0],409)

if __name__ == "__main__":
    unittest.main()
