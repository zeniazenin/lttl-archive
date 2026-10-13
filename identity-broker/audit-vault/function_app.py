"""Azure Functions Python v2 entry point."""
import json
import azure.functions as func
from vault import handle

app = func.FunctionApp(http_auth_level=func.AuthLevel.ANONYMOUS)

@app.route(route="audit", methods=["POST"])
def ingest(request: func.HttpRequest) -> func.HttpResponse:
    status, result = handle(request.method, dict(request.headers), request.get_body())
    return func.HttpResponse(json.dumps(result), status_code=status, mimetype="application/json", headers={"Cache-Control": "no-store"})
