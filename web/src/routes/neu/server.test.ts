import { describe, expect, it, vi } from "vitest";
import { isActionFailure, isRedirect } from "@sveltejs/kit";
import { actions, load } from "./+page.server";

/** The create form: a real form action, and an API that decides everything. */

function spyFetch(answers: { status: number; body?: unknown }[]) {
  const calls: { url: string; method: string; body?: string }[] = [];
  let next = 0;
  const fetchFn = vi.fn(
    async (url: string | URL | Request, init?: RequestInit) => {
      const a = answers[Math.min(next++, answers.length - 1)];
      calls.push({
        url: String(url),
        method: init?.method ?? "GET",
        body: init?.body as string,
      });
      return new Response(a.body === undefined ? "" : JSON.stringify(a.body), {
        status: a.status,
      });
    },
  );
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function actionEvent(
  fetchFn: typeof fetch,
  fields: Record<string, string>,
): any {
  const form = new FormData();
  for (const [k, v] of Object.entries(fields)) form.append(k, v);
  return {
    fetch: fetchFn,
    request: new Request("http://wiki.test/neu", {
      method: "POST",
      headers: { cookie: "gw_session=abc" },
      body: form,
    }),
  };
}

async function run(fetchFn: typeof fetch, fields: Record<string, string>) {
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return {
      returned: await (actions.anlegen as any)(actionEvent(fetchFn, fields)),
      thrown: null,
    };
  } catch (e) {
    return { returned: null, thrown: e };
  }
}

describe("the picker", () => {
  it("lists exactly what the API answered, with the cookie", async () => {
    const list = [{ path: "/vorlagen/a", title: "A" }];
    const { calls, fetchFn } = spyFetch([{ status: 200, body: list }]);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (await (load as any)({
      fetch: fetchFn,
      request: new Request("http://wiki.test/neu", {
        headers: { cookie: "gw_session=abc" },
      }),
      url: new URL("http://wiki.test/neu?unter=%2Fraum"),
    })) as any;
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toContain("/api/templates");
    expect(data.vorlagen).toEqual(list);
    expect(data.unter).toBe("/raum");
  });

  it("shows an empty picker, not an error, when the list cannot be had", async () => {
    const { fetchFn } = spyFetch([{ status: 500 }]);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (await (load as any)({
      fetch: fetchFn,
      request: new Request("http://wiki.test/neu"),
      url: new URL("http://wiki.test/neu"),
    })) as any;
    expect(data.vorlagen).toEqual([]);
  });
});

describe("the anlegen action", () => {
  it("posts title, parent and template, then goes to the new page in the editor", async () => {
    const { calls, fetchFn } = spyFetch([
      { status: 201, body: { path: "/raum/montag" } },
    ]);
    const { thrown } = await run(fetchFn, {
      titel: " Montag ",
      unter: "/raum",
      vorlage: "/vorlagen/protokoll",
    });
    expect(calls[0].method).toBe("POST");
    expect(calls[0].url).toContain("/api/pages");
    expect(JSON.parse(calls[0].body ?? "{}")).toEqual({
      parent: "/raum",
      title: "Montag",
      template: "/vorlagen/protokoll",
    });
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { location: string }).location).toBe(
      "/raum/montag?edit=1",
    );
  });

  it("sends blanks as no parent and no template", async () => {
    const { calls, fetchFn } = spyFetch([
      { status: 201, body: { path: "/x" } },
    ]);
    await run(fetchFn, { titel: "X", unter: "", vorlage: "" });
    expect(JSON.parse(calls[0].body ?? "{}")).toEqual({
      parent: null,
      title: "X",
      template: null,
    });
  });

  it("refuses an empty title without asking the API", async () => {
    const { calls, fetchFn } = spyFetch([
      { status: 201, body: { path: "/x" } },
    ]);
    const { returned } = await run(fetchFn, {
      titel: "  ",
      unter: "",
      vorlage: "",
    });
    expect(calls).toHaveLength(0);
    expect(isActionFailure(returned)).toBe(true);
  });

  it("shows the API’s own refusal, keeps what was typed, and keeps the status", async () => {
    const { fetchFn } = spyFetch([
      { status: 409, body: { error: "you may not add pages under /vorlagen" } },
    ]);
    const { returned } = await run(fetchFn, {
      titel: "T",
      unter: "/vorlagen",
      vorlage: "",
    });
    expect(isActionFailure(returned)).toBe(true);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const f = returned as any;
    expect(f.status).toBe(409);
    expect(f.data.fehler).toContain("you may not add pages under /vorlagen");
    expect(f.data.titel).toBe("T");
    expect(f.data.unter).toBe("/vorlagen");
  });
});
