package com.tangluobo.rdp4j;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HexFormat;

/** Runs the original read-only Java decoder; input/output are owned fixture files. */
public final class RdpClearCodecOracle {
    public static void main(String[] args) throws Exception {
        ClearCodecDecoder decoder = new ClearCodecDecoder();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            String[] fields = line.split("\t");
            int width = Integer.parseInt(fields[1]);
            int height = Integer.parseInt(fields[2]);
            byte[] stream = HexFormat.of().parseHex(fields[3]);
            int[] pixels = new int[width * height];
            decoder.decode(stream, 0, stream.length, width, height, pixels, width, 0, 0);
            byte[] bgra = new byte[pixels.length * 4];
            for (int i = 0; i < pixels.length; i++) {
                bgra[4 * i] = (byte)pixels[i];
                bgra[4 * i + 1] = (byte)(pixels[i] >>> 8);
                bgra[4 * i + 2] = (byte)(pixels[i] >>> 16);
                bgra[4 * i + 3] = (byte)255;
            }
            System.out.println(fields[0] + "\t" + HexFormat.of().formatHex(bgra));
        }
    }
}
