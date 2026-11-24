import com.tangluobo.tomato.ssh.SSHTerminalPane;
import javafx.application.Application;
import javafx.application.Platform;
import javafx.animation.PauseTransition;
import javafx.util.Duration;
import javafx.scene.Scene;
import javafx.scene.control.Button;
import javafx.scene.image.WritableImage;
import javafx.stage.Stage;
import java.awt.image.BufferedImage;
import javax.imageio.ImageIO;
import java.io.File;

/** Manual visual oracle. Only connects to the supplied disposable loopback port. */
public final class LegacySshCapture {
    public static void main(String[] args) { Application.launch(Capture.class,args); }
    public static final class Capture extends Application {
        @Override public void start(Stage stage) {
            System.err.println("capture: JavaFX ready");
            var args=getParameters().getRaw();int port=Integer.parseInt(args.get(0));
            System.err.println("capture: constructing terminal");
            SSHTerminalPane pane=new SSHTerminalPane();System.err.println("capture: terminal constructed");Scene scene=new Scene(pane,896,846);
            stage.setTitle("Tomato 原版 SSH 隔离验收");stage.setScene(scene);
            if(!Boolean.getBoolean("tomato.capture.offscreen")) stage.show();
            pane.resize(896,846);pane.applyCss();pane.layout();
            new Thread(()->{try{
                System.err.println("capture: connecting fixture");pane.connect("127.0.0.1",port,"fixture","fixture-only");System.err.println("capture: fixture connected");
                Platform.runLater(()->{
                    for(var node:pane.lookupAll(".button"))if(node instanceof Button button && button.getTooltip()!=null && java.util.Set.of("文件","监控","端口","Docker").contains(button.getTooltip().getText())){System.err.println("capture: tool "+button.getTooltip().getText());button.fire();}
                    System.err.println("capture: tools ready");
                    PauseTransition wait=new PauseTransition(Duration.seconds(3));wait.setOnFinished(e->{try{
                        pane.applyCss();pane.layout();WritableImage pixels=scene.snapshot(null);int w=(int)pixels.getWidth(),h=(int)pixels.getHeight();
                        BufferedImage image=new BufferedImage(w,h,BufferedImage.TYPE_INT_ARGB);
                        int[] argb=new int[w*h];pixels.getPixelReader().getPixels(0,0,w,h,javafx.scene.image.PixelFormat.getIntArgbInstance(),argb,0,w);
                        image.setRGB(0,0,w,h,argb,0,w);ImageIO.write(image,"png",new File(args.get(1)));
                        pane.disconnect();System.exit(0);
                    }catch(Throwable error){error.printStackTrace();System.exit(1);}});wait.play();
                });
            }catch(Throwable error){error.printStackTrace();System.exit(1);}},"legacy-loopback-capture").start();
        }
    }
}
