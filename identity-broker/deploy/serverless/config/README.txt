Put the chosen provider's public broker configuration here as broker.json before
building a Lambda image. Cloud Run may instead mount broker.json from a secret
volume. Optional database CA certificates may be placed here too.

NEVER put credentials, private keys, .env files or a database URL in this folder.
Everything in this folder is copied into the deployed image.
