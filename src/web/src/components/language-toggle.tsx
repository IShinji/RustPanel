import { Languages } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { useLocale } from "@/lib/i18n/locale-provider";

export function LanguageToggle() {
  const { locale, setLocale, t } = useLocale();

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={t("components.languageToggleLabel")}>
          <Languages className="size-4" />
          <span className="sr-only">{t("components.languageToggleLabel")}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {/* 每种语言永远用它自己的文字写,不查字典——看不懂当前语言的人也能找到自己的语言。 */}
        <DropdownMenuItem
          onClick={() => setLocale("zh-CN")}
          data-active={locale === "zh-CN"}
          className="data-[active=true]:bg-accent"
        >
          <span>简体中文</span> {/* i18n-ignore: 语言名用母语自己写,不查字典 */}
        </DropdownMenuItem>
        <DropdownMenuItem
          onClick={() => setLocale("en")}
          data-active={locale === "en"}
          className="data-[active=true]:bg-accent"
        >
          <span>English</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
