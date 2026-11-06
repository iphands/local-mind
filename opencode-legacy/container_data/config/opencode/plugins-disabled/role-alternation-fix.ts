import type { Plugin } from "@opencode/plugin"
import { appendFileSync } from "fs"
import { homedir } from "os"
import { join } from "path"

const logFile = join(homedir(), ".local", "my_opencode.log")

function mlog(msg: string) {
  appendFileSync(logFile, `[${new Date().toISOString()}] ${msg}\n`)
}

const plugin: Plugin = async (input) => {
  return {
    "experimental.chat.messages.transform": async (_input, output) => {

      // Fix role alternation issues for llama.cpp Jinja templates
      // which require strict user/assistant alternation
      const result = []

      for (let i = 0; i < output.messages.length; i++) {
        const msg = output.messages[i]
        const prev = result[result.length - 1]

        // System messages allowed at start
        if (msg.info.role === "system") {
          result.push(msg)
          continue
        }

        // Tool messages allowed after assistant
        if (msg.info.role === "tool") {
          result.push(msg)
          continue
        }

        // If user follows user, insert placeholder assistant
        if (msg.info.role === "user" && prev?.info.role === "user") {
          mlog("WARN: user, insert placeholder assistant");
          result.push({
            info: { role: "assistant" },
            parts: [{ type: "text", text: "Understood." }]
          })
        }

        // If user follows tool, insert placeholder assistant
        if (msg.info.role === "user" && prev?.info.role === "tool") {
          mlog("WARN: follows tool, insert placeholder assistant");
          result.push({
            info: { role: "assistant" },
            parts: [{ type: "text", text: "Done." }]
          })
        }

        // If assistant follows assistant, insert placeholder user
        if (msg.info.role === "assistant" && prev?.info.role === "assistant") {
          mlog("WARN: follows assistant, insert placeholder user");
          result.push({
            info: { role: "user" },
            parts: [{ type: "text", text: "Continue." }]
          })
        }

        result.push(msg)
      }

      output.messages = result
    }
  }
}

export default plugin
