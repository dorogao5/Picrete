# Public domains

`picrete.com` and `picrete.ru` serve the same Front and API release. `dev.picrete.com` and `dev.picrete.ru` serve the same Studio release. Each www hostname redirects to the apex in its own zone. Authentication is browser-origin scoped; signing in on .com does not automatically sign in on .ru.

The existing .com sites retain their certificates. Install `nginx-picrete.ru.conf` as `/etc/nginx/sites-enabled/picrete.ru` only after issuing the `picrete.ru` certificate with SANs `picrete.ru`, `www.picrete.ru`, `dev.picrete.ru`. HTTP ACME validation uses `/var/www/html`. The paths reference the existing production release symlinks and shared immutable assets.

Install `certbot-reload-nginx.sh` with mode 0755 as `/etc/letsencrypt/renewal-hooks/deploy/reload-nginx`. Keep certbot.timer enabled. Validate renewal with `certbot renew --cert-name picrete.ru --dry-run --run-deploy-hooks`.

Preserve the existing production CORS values and add `https://picrete.ru` and `https://www.picrete.ru` to Picrete BACKEND_CORS_ORIGINS; add `https://dev.picrete.ru` to STUDIO_CORS_ORIGINS. These are environment settings, not committed credentials. Apply using the existing Compose project names and SHA-tagged images.

When changing nginx application routes, keep both .com templates and the .ru mirrors aligned. Deployment is complete only after nginx -t, HTTPS/version/build-info checks on both zones, and renewal verification. DNS A/AAAA must match reachable interfaces; mail and smtp records do not create a mail service.
