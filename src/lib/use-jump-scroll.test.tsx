import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useJumpScroll } from "@/lib/use-jump-scroll";

/** 最小宿主：复刻页面的用法——ref 只挂在当前选中行上 */
function Harness({
  rows,
  activeKey,
  seq,
  jumpKey,
}: {
  rows: string[];
  activeKey: string | null;
  seq: number;
  jumpKey: string | null;
}) {
  const [active, setActive] = useState(activeKey);
  const row = useJumpScroll<HTMLLIElement>({
    activeKey: active,
    seq,
    jumpKey,
  });
  return (
    <ul>
      {rows.map((r) => (
        <li key={r} ref={r === active ? row : undefined}>
          <button type="button" onClick={() => setActive(r)}>
            {r}
          </button>
        </li>
      ))}
    </ul>
  );
}

describe("useJumpScroll", () => {
  const rows = ["alpha", "beta", "gamma"];

  it("定位跳转让目标行居中", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    render(<Harness rows={rows} activeKey="beta" seq={1} jumpKey="beta" />);

    const target = (
      await screen.findByRole("button", { name: "beta" })
    ).closest("li");
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(scrollIntoView.mock.instances[0]).toBe(target);
  });

  it("同一个键再跳一次仍会居中", () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    const { rerender } = render(
      <Harness rows={rows} activeKey="beta" seq={1} jumpKey="beta" />,
    );
    scrollIntoView.mockClear();

    rerender(<Harness rows={rows} activeKey="beta" seq={2} jumpKey="beta" />);
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
  });

  it("点击行只做最小滚动", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    render(<Harness rows={rows} activeKey="alpha" seq={0} jumpKey={null} />);

    fireEvent.click(await screen.findByRole("button", { name: "beta" }));
    expect(scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
  });

  it("跳转目标与选中行不一致时按最小滚动", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    render(<Harness rows={rows} activeKey="beta" seq={1} jumpKey="gamma" />);

    await screen.findByRole("button", { name: "beta" });
    expect(scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
  });
});
