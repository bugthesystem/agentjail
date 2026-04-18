import { api } from "@/lib/api";
import { CredentialCard } from "@/components/CredentialCard";
import { EmptyState } from "@/components/EmptyState";
import { PageHeader } from "@/components/PageHeader";

export default async function CredentialsPage() {
  const credentials = await api.credentials.list().catch(() => []);
  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader
        title="Credentials"
        subtitle="Real API keys. Never sent to any sandbox."
      />
      {credentials.length === 0 ? (
        <EmptyState
          title="No credentials configured"
          hint="Attach one via POST /v1/credentials or aj.credentials.put({...})."
        />
      ) : (
        <div className="flex flex-col gap-3">
          {credentials.map((c) => (
            <CredentialCard key={c.service} rec={c} />
          ))}
        </div>
      )}
    </div>
  );
}
