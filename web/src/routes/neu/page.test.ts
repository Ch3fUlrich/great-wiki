import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import Page from "./+page.svelte";

describe("the create form, rendered without a script", () => {
  const html = (props: object) =>
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    render(Page as any, { props }).body;

  it("is a POST form to the action, with labelled native controls", () => {
    const out = html({
      data: {
        vorlagen: [{ path: "/vorlagen/a", title: "Protokoll" }],
        unter: "/raum",
      },
      form: null,
    });
    expect(out).toContain('method="POST"');
    expect(out).toContain('action="?/anlegen"');
    expect(out).toContain('for="neu-titel"');
    expect(out).toContain('for="neu-vorlage"');
    expect(out).toContain('<option value="/vorlagen/a"');
    expect(out).toContain("Leere Seite");
    expect(out).toContain('value="/raum"');
  });

  it("shows a refusal as an alert", () => {
    const out = html({
      data: { vorlagen: [], unter: "" },
      form: {
        fehler: "Die Seite wurde nicht angelegt.",
        titel: "T",
        unter: "",
        vorlage: "",
      },
    });
    expect(out).toContain('role="alert"');
    expect(out).toContain("Die Seite wurde nicht angelegt.");
  });
});
