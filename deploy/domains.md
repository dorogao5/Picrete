# Public domains

`https://picrete.com` is the canonical Picrete address. `https://dev.picrete.com` is the canonical Studio address. Use these domains in documentation, service registration, links and ITMO.ID configuration. The production ITMO.ID callback is `https://picrete.com/auth/itmo/callback`.

`picrete.ru` is a defensive fallback domain. Page requests on its apex and www host redirect permanently to `https://picrete.com`, preserving the path and query string. `dev.picrete.ru` redirects pages to `https://dev.picrete.com`. HTTPS API, health/version and immutable asset routes remain available for already-open .ru browser sessions. Do not initiate ITMO.ID login on .ru. Authentication storage belongs to each origin; visitors previously signed in on .ru sign in again on .com.

Keep the .ru DNS records and certificate active. Install `nginx-picrete.ru.conf` as `/etc/nginx/sites-enabled/picrete.ru`; the `picrete.ru` certificate must cover `picrete.ru`, `www.picrete.ru` and `dev.picrete.ru`. HTTP ACME validation uses `/var/www/html` and is exempt from redirects. The existing .com sites retain their certificates.

Install `certbot-reload-nginx.sh` with mode 0755 as `/etc/letsencrypt/renewal-hooks/deploy/reload-nginx`. Keep certbot.timer enabled. After certificate or renewal configuration changes, validate with `certbot renew --cert-name picrete.ru --dry-run --run-deploy-hooks`.

Preserve production CORS entries for existing .ru sessions while the compatibility API routes are in use. Environment settings contain credentials and are not committed. Apply using the existing Compose project names and SHA-tagged images.

Before reloading nginx, run `nginx -t`. Verify .com pages, API readiness, frontend assets and ITMO configuration, then verify .ru redirects with nested paths and query strings. Keep compatible API routes aligned with the .com templates. DNS A/AAAA must match reachable interfaces; mail and smtp records do not create a mail service.
