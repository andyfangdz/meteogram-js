import path from "path";
import type { Config } from "tailwindcss";
import { heroui } from "@heroui/react";
const nexUiThemePath = path.dirname(require.resolve("@heroui/theme"));

const config: Config = {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
    `${nexUiThemePath}/**/*.{js,ts,jsx,tsx}`,
  ],
  theme: {
    extend: {
      backgroundImage: {
        "gradient-radial": "radial-gradient(var(--tw-gradient-stops))",
        "gradient-conic":
          "conic-gradient(from 180deg at 50% 50%, var(--tw-gradient-stops))",
      },
    },
  },
  darkMode: "class",
  plugins: [heroui()],
};
export default config;
