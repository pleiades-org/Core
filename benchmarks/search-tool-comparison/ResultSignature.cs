using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Text;

public sealed class ResultSignature
{
    public int Count { get; private set; }
    public string Hash { get; private set; }

    public static ResultSignature Oracle(string directory, string query)
    {
        var output = new StringBuilder();
        foreach (string path in Directory.GetFiles(directory, "*.txt"))
        {
            int lineNumber = 0;
            foreach (string line in File.ReadLines(path, Encoding.UTF8))
            {
                lineNumber++;
                if (line.Contains(query, StringComparison.Ordinal))
                    output.Append(Path.GetFileName(path)).Append(':').Append(lineNumber).Append(':').Append(line).Append('\n');
            }
        }
        return Parse(output.ToString(), false);
    }

    public static ResultSignature Parse(string output, bool originalSearch)
    {
        var canonical = new List<string>();
        foreach (string rawLine in output.Split('\n', StringSplitOptions.RemoveEmptyEntries))
        {
            string line = rawLine.TrimEnd('\r');
            int filenameEnd = line.IndexOf(".txt:", StringComparison.Ordinal);
            if (filenameEnd < 0) throw new FormatException("Missing fixture filename in output.");
            int filenameStart = line.LastIndexOfAny(new[] {'/', '\\'}, filenameEnd) + 1;
            int contentStart = line.IndexOf(':', filenameEnd + 5) + 1;
            if (contentStart == 0) throw new FormatException("Missing line number in output.");
            string prefix = line.Substring(filenameStart, contentStart - filenameStart);
            if (originalSearch)
            {
                if (line[contentStart] != ' ') throw new FormatException("Search separator changed.");
                contentStart++;
            }
            canonical.Add(prefix + line.Substring(contentStart));
        }
        canonical.Sort(StringComparer.Ordinal);
        return new ResultSignature {
            Count = canonical.Count,
            Hash = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("\n", canonical))))
        };
    }
}
