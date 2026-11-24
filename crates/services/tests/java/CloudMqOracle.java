import javafx.application.*;
import javafx.stage.*;
import javafx.scene.*;
import javafx.scene.layout.*;
import javafx.animation.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.*;
import com.tangluobo.tomato.module.connect.view.*;
/** Current original MQ views, their own isolated loopback brokers and scenes only. */
public class CloudMqOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var args=getParameters().getRaw();String kind=args.get(0),mode=args.get(1),endpoint=args.get(2);Path out=Path.of(args.get(3));Files.createDirectories(out);
   if(!System.getProperty("user.home").contains("oracle-cloud-mq-")||!endpoint.startsWith("127.0.0.1:"))throw new IllegalStateException("Owned loopback fixtures required");
   ConnectionConfig c=new ConnectionConfig();c.setId("owned-oracle");c.setName("隔离"+kind);c.setType(ConnectType.valueOf(kind));c.setHost("127.0.0.1");c.setPort(Integer.parseInt(endpoint.substring(endpoint.indexOf(':')+1)));
   Parent pane;
   if(kind.equals("KAFKA")){var view=new KafkaDataView(c,"owned-oracle");if(mode.equals("messages"))view.selectMessageTab("owned-oracle");if(mode.equals("send"))view.selectSendTab("owned-oracle");pane=view;}
   else{var view=new RocketmqDataView(c,"owned-oracle");if(mode.equals("messages"))view.selectMessageTab("owned-oracle");if(mode.equals("send"))view.selectSendTab("owned-oracle");pane=view;}
   stage.setScene(new Scene(pane,895,846));stage.setOpacity(.01);stage.show();stage.toBack();
   var delay=new PauseTransition(Duration.seconds(2));delay.setOnFinished(e->{try{
    pane.applyCss();pane.layout();Path image=out.resolve(kind.toLowerCase()+"-"+mode+".png");ImageIO.write(SwingFXUtils.fromFXImage(stage.getScene().snapshot(null),null),"png",image.toFile());
    var nodes=new JsonArray();ToolPaneOracle.nodes(pane,nodes);Files.writeString(Path.of(image.toString()+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(nodes));stage.close();Platform.exit();System.exit(0);
   }catch(Exception ex){ex.printStackTrace();System.exit(1);}});delay.play();
  }
 }
}