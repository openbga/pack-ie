import { act, screen } from "@testing-library/react";
import { useParams } from "react-router";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/Button";
import { StatusBadge } from "@/components/StatusBadge";
import { expectNoA11yViolations } from "@/lib/test/a11y";
import { renderWithProviders } from "@/lib/test/render";
import { renderRoutes } from "@/lib/test/router";
import { Page } from "./Page";
import { PageHeader } from "./PageHeader";

describe("Page", () => {
  it("has one h1 with its description and actions", async () => {
    const { container } = renderWithProviders(
      <Page>
        <PageHeader
          title="Worklist"
          description="Synthetic rows"
          actions={<Button>Refresh</Button>}
        />
        <p>Content</p>
      </Page>,
    );
    expect(screen.getAllByRole("heading", { level: 1 }).map((h) => h.textContent)).toEqual([
      "Worklist",
    ]);
    expect(screen.getByText("Synthetic rows")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeTruthy();
    expect(screen.queryByRole("navigation")).toBeNull();
    await expectNoA11yViolations(container);
  });
});

function RecordHeader({ sequence }: { sequence: "middle" | "first" }) {
  const next = { href: "/tests/SYN-B", routerOptions: { replace: true } };
  return (
    <PageHeader
      title="SYN-A Synthetic panel A"
      breadcrumbs={[
        { label: "Tests and panels", href: "/tests?q=syn" },
        { label: "SYN-A", href: "/tests/SYN-A" },
      ]}
      status={<StatusBadge tone="warning">Draft awaiting approval</StatusBadge>}
      sequence={
        sequence === "middle"
          ? { previous: { href: "/tests/SYN-0" }, next }
          : { previous: null, next }
      }
      actions={<Button>Edit draft</Button>}
    />
  );
}

describe("PageHeader on a record page", () => {
  it("keeps breadcrumb identity when a loaded parent is inserted", () => {
    const current = { label: "Accession", href: "/accessions/synthetic" };
    const { rerender } = renderWithProviders(
      <PageHeader title="Accession" breadcrumbs={[current]} />,
    );
    rerender(
      <PageHeader
        title="Accession"
        breadcrumbs={[{ label: "Order", href: "/orders/synthetic" }, current]}
      />,
    );
    expect(screen.getByRole("link", { name: "Order" })).toBeTruthy();
    expect(
      screen.getByRole("navigation", { name: "Breadcrumb" }).querySelector("[aria-current=page]")
        ?.textContent,
    ).toBe("Accession");
  });

  it("shows the breadcrumb to the list, ending at this page", async () => {
    const { container, user, router } = renderRoutes(
      [{ path: "/tests/SYN-A", element: <RecordHeader sequence="middle" /> }],
      "/tests/SYN-A",
    );
    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(breadcrumb.textContent).toBe("Tests and panelsSYN-A");
    expect(screen.getByText("SYN-A").closest("[aria-current=page]")).toBeTruthy();
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("SYN-A Synthetic panel A");
    expect(screen.getByText("Draft awaiting approval")).toBeTruthy();
    await expectNoA11yViolations(container);

    await user.click(screen.getByRole("link", { name: "Tests and panels" }));
    expect(router.state.location.pathname).toBe("/tests");
    expect(router.state.location.search).toBe("?q=syn");
  });

  it("steps to the next record with the link's router options", async () => {
    const { user, router } = renderRoutes(
      [{ path: "/tests/SYN-A", element: <RecordHeader sequence="middle" /> }],
      "/tests/SYN-A",
    );
    const sequence = screen.getByRole("navigation", { name: "Records in list order" });
    expect(sequence.textContent).toBe("PreviousNext");
    await user.click(screen.getByRole("link", { name: "Next" }));
    expect(router.state.location.pathname).toBe("/tests/SYN-B");
    expect(router.state.historyAction).toBe("REPLACE");
  });

  it("keeps focus on Next when it steps onto the last record", async () => {
    function Stepper() {
      const { id = "" } = useParams();
      const index = Number(id);
      const link = (to: number) =>
        to < 0 || to > 2 ? null : { href: `/records/${to}`, routerOptions: { replace: true } };
      return (
        <PageHeader
          title={`Record ${id}`}
          sequence={{ previous: link(index - 1), next: link(index + 1) }}
        />
      );
    }
    const { user } = renderRoutes([{ path: "/records/:id", element: <Stepper /> }], "/records/1");
    const next = screen.getByRole("link", { name: "Next" });
    act(() => next.focus());
    await user.keyboard("{Enter}");
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Record 2");
    expect(document.activeElement).toBe(next);
    expect(next.getAttribute("aria-disabled")).toBe("true");
    // Disabled, it ignores activation and keeps focus.
    await user.keyboard("{Enter}");
    await user.click(next);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Record 2");
    expect(document.activeElement).toBe(next);
  });

  it("keeps Previous in place, disabled, at the start of the list", () => {
    renderRoutes(
      [{ path: "/tests/SYN-A", element: <RecordHeader sequence="first" /> }],
      "/tests/SYN-A",
    );
    const previous = screen.getByRole("link", { name: "Previous" });
    expect(previous.getAttribute("aria-disabled")).toBe("true");
    expect(previous.hasAttribute("href")).toBe(false);
    expect(screen.getByRole("link", { name: "Next" }).getAttribute("aria-disabled")).toBeNull();
  });
});
