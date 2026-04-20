import { DocPage, Section, Inline, Hint } from "../../components/docs/DocPage";
import { Code } from "../../components/docs/Code";

export function Gateway() {
  return (
    <DocPage
      eyebrow="Concepts"
      title="Gateway (inbound hostname routing)"
      lead={
        <>
          Attach one or more public hostnames to a workspace; the server
          runs a plain HTTP reverse proxy that forwards matching traffic
          to a caller-supplied backend URL. Lean on purpose — no TLS, no
          wildcard DNS, no jail-internal IP discovery. Pair with Docker
          networking, a side-car tunnel, or upstream TLS termination.
        </>
      }
    >
      <Section id="enable" title="Enable">
        <p>
          Set <Inline>AGENTJAIL_GATEWAY_ADDR</Inline> on the server and
          declare <Inline>domains</Inline> on a workspace:
        </p>
        <Code lang="bash">{`# agentjail-server
export AGENTJAIL_GATEWAY_ADDR=0.0.0.0:8080`}</Code>
        <Code lang="ts">{`const ws = await aj.workspaces.create({
  label: "preview",
  domains: [
    { domain: "review-42.preview.local", backend_url: "http://127.0.0.1:3000" },
  ],
});`}</Code>
      </Section>

      <Section id="how" title="How matching works">
        <p>
          On every incoming request, the gateway trims the <Inline>:port</Inline>
          suffix from the <Inline>Host</Inline> header and compares
          case-insensitively against each workspace&rsquo;s{" "}
          <Inline>domains[].domain</Inline>. Hop-by-hop headers are stripped;
          bodies under 10&nbsp;MB are forwarded as-is. An{" "}
          <Inline>X-Forwarded-Host</Inline> header is set to the original hostname.
        </p>
        <Code lang="text">{`client                                   agentjail-server                       backend
  │                                      │                                       │
  │ GET / HTTP/1.1                       │                                       │
  │ Host: review-42.preview.local:8080   │                                       │
  ├─────────────────────────────────────▶│                                       │
  │                                      │ lookup workspace.domains              │
  │                                      │ forward to backend_url + path+query   │
  │                                      ├─────────────────────────────────────▶│
  │                                      │                                       │
  │                                      │◀──────────────────────────────────────│
  │◀─────────────────────────────────────│                                       │`}</Code>
        <Hint title="Unmatched hosts" tone="phantom">
          Requests whose <Inline>Host</Inline> doesn&rsquo;t match any live
          workspace domain get a plain-text <Inline>404</Inline> naming the
          host. Missing-header requests get a <Inline>400</Inline>.
        </Hint>
      </Section>

      <Section id="limits" title="What&rsquo;s intentionally out of scope">
        <ul className="list-disc pl-5 marker:text-ink-600 space-y-1.5 text-[14px] text-ink-300">
          <li>TLS termination — run upstream (nginx, Caddy, cloud LB) and forward plain HTTP.</li>
          <li>Wildcard DNS — domains are exact matches; add one row per subdomain.</li>
          <li>Jail-internal IP discovery — you supply <Inline>backend_url</Inline>.</li>
          <li>Per-workspace rate limits / auth — add at the upstream layer.</li>
        </ul>
        <p>
          These are deliberate omissions to keep the gateway lean. Each one
          is a well-understood add-on for a later iteration.
        </p>
      </Section>
    </DocPage>
  );
}
