import java.io.*;
import java.nio.file.*;
public class ObjectStringGolden {
  public static void main(String[] args) throws Exception {
    Path root = Path.of(args[0]); Files.createDirectories(root);
    for (String name : new String[]{"short", "long"}) {
      String text = name.equals("short") ? "fixture\nS3\n目录\nF|🍅.txt\n\0" : "x".repeat(70000);
      try (ObjectOutputStream out = new ObjectOutputStream(Files.newOutputStream(root.resolve("java-string-" + name + ".bin")))) { out.writeObject(text); }
    }
  }
}
