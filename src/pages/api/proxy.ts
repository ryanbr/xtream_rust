// src/pages/api/proxy.ts
import type { APIRoute } from "astro";

export const GET: APIRoute = async ({ request }) => {
    try {
        const reqUrl = new URL(request.url, "http://localhost"); // handles relative URL
        const target = reqUrl.searchParams.get("url");
        if (!target) return new Response("Missing ?url=", { status: 400 });

        const upstream = await fetch(target, { redirect: "follow" });

        const headers = new Headers();
        copy(upstream.headers, headers, "content-type");
        headers.set("Cache-Control", "no-cache");
        headers.set("Access-Control-Allow-Origin", reqUrl.origin);

        if (!upstream.ok) {
            // Read the body ONCE and return it; do NOT return upstream.body after this
            const text = await upstream.text().catch(() => "");
            return new Response(text || `Upstream ${upstream.status}`, {
                status: upstream.status,
                headers,
            });
        }

        // Success path: stream through (we haven’t touched the body)
        return new Response(upstream.body, { status: upstream.status, headers });
    } catch (e: any) {
        return new Response(`Proxy error: ${e.message}`, { status: 502 });
    }
};

function copy(from: Headers, to: Headers, name: string) {
    const v = from.get(name);
    if (v) to.set(name, v);
}
