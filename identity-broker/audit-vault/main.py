"""Google Cloud Run functions entry point."""
from vault import gcp_handler

def ingest(request):
    return gcp_handler(request)
