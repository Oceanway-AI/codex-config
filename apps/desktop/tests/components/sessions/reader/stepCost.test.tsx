import { screen } from "@testing-library/react";
import { renderWithQueryClient as render } from "../../../utils/testQueryClient";
import { describe, expect, it } from "vitest";
import { getAgentReaderStyle } from "@/components/sessions/reader/agentStyles";
import { transcriptToMarkdown } from "@/components/sessions/reader/exportMarkdown";
import { StepMetaText } from "@/components/sessions/reader/rowStyles";
import { buildTurns } from "@/components/sessions/reader/turns";
import { formatMoneyCompact, USD_DISPLAY } from "@/components/usage/format";
import { fixtures } from "./helpers";

// Oceanway 不展示用量统计；导出历史时仍保留原记录中的费用数据。
describe("session reader step cost", () => {
  it("hides usage metadata in the Oceanway session reader", () => {
    const { container } = render(
      <StepMetaText
        meta={{
          status: "success",
          glyph: "●",
          tone: "success",
          tokens: 28_500,
          costUsd: 0.0123,
        }}
      />,
    );
    expect(screen.queryByText("$0.012")).not.toBeInTheDocument();
    expect(container).toBeEmptyDOMElement();
  });

  it("writes step costs into the exported markdown with the given formatter", () => {
    const style = getAgentReaderStyle("opencode");
    const markdown = transcriptToMarkdown(
      "OpenCode",
      buildTurns(fixtures.opencode, { style }),
      {
        style,
        appName: "OpenCode",
        youLabel: "You",
        t: (key) => key,
        formatCost: (usd) => formatMoneyCompact(usd, USD_DISPLAY),
      },
    );
    expect(markdown).toContain("$0.0081");
  });
});
