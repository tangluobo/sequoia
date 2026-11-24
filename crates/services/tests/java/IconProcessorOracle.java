package com.tangluobo.tomato.module.tools;
import java.awt.image.BufferedImage;
import java.nio.file.*;
import javax.imageio.ImageIO;
public final class IconProcessorOracle {
    public static void main(String[] args) throws Exception {
        Path output=Path.of(args[0]);Files.createDirectories(output);
        for(int fixture=0;fixture<4;fixture++) {
            BufferedImage image=new BufferedImage(73,61,BufferedImage.TYPE_INT_ARGB);
            int bg=new int[]{0xffffffff,0xff000000,0xff2487ca,0xffffffff}[fixture];
            for(int y=0;y<61;y++) for(int x=0;x<73;x++) {
                int value=bg;
                double d=Math.hypot((x-36)*1.0,(y-30)*1.2);
                if(d<18) value=0xffb82245;
                else if(d<22) {
                    double a=(22-d)/4;
                    int r=(int)Math.round(((bg>>16)&255)*(1-a)+184*a);
                    int g=(int)Math.round(((bg>>8)&255)*(1-a)+34*a);
                    int b=(int)Math.round((bg&255)*(1-a)+69*a);
                    value=0xff000000|(r<<16)|(g<<8)|b;
                }
                if(fixture==3 && x<5 && y<5) value=0xffb82245;
                image.setRGB(x,y,value);
            }
            ImageIO.write(image,"png",output.resolve("source-"+fixture+".png").toFile());
            for(boolean smooth:new boolean[]{false,true}) for(boolean crop:new boolean[]{false,true}) {
                BufferedImage result=IconImageProcessor.process(image,20,crop,smooth,2);
                ImageIO.write(result,"png",output.resolve("expected-"+fixture+"-"+(smooth?1:0)+"-"+(crop?1:0)+".png").toFile());
            }
        }
    }
}
