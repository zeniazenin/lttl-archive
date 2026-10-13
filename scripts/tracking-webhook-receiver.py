"""Local webhook verification example. Put production receivers behind HTTPS.

Set LTTL_TRACKING_WEBHOOK_SECRET in the environment. This example listens only
on loopback, verifies signatures, and prints verified events for integration
testing. Replace in-memory deduplication/printing with durable SIEM ingestion.
"""
import base64
import hashlib
import hmac
import http.server
import json
import os
import time

SECRET=os.environ['LTTL_TRACKING_WEBHOOK_SECRET'].encode()
if len(SECRET)<32:raise SystemExit('Webhook secret must have at least 32 bytes')
SEEN={}

class Receiver(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def do_POST(self):
        try:
            length=int(self.headers.get('Content-Length','0'))
            if not 0<length<=65536:raise ValueError()
            body=self.rfile.read(length)
            event_id=self.headers['X-Lttl-Event-Id'];stamp=self.headers['X-Lttl-Timestamp']
            if len(event_id)!=43 or abs(time.time()-int(stamp))>300:raise ValueError()
            expected=base64.urlsafe_b64encode(hmac.new(SECRET,event_id.encode()+b'.'+stamp.encode()+b'.'+body,hashlib.sha256).digest()).decode().rstrip('=')
            if not hmac.compare_digest(expected,self.headers.get('X-Lttl-Signature','')):raise ValueError()
            event=json.loads(body)
            if event['event_id']!=event_id or event['schema']!=1:raise ValueError()
        except (ValueError,KeyError,TypeError):
            self.send_response(403);self.end_headers();return
        for key in [key for key,expiry in SEEN.items() if expiry<time.time()]:del SEEN[key]
        if event_id not in SEEN:
            print(json.dumps(event),flush=True)
            SEEN[event_id]=time.time()+90*86400
        self.send_response(204);self.end_headers()

if __name__=='__main__':
    http.server.HTTPServer(('127.0.0.1',8790),Receiver).serve_forever()
