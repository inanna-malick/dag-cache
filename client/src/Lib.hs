{-# LANGUAGE DataKinds #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE TypeOperators #-}
{-# LANGUAGE DeriveGeneric #-}

module Lib where

import Data.Aeson
import Data.ByteString
import Data.Proxy
import Data.Text
import Network.HTTP.Client (newManager, defaultManagerSettings)
import Servant.API
import Servant.Client

import GHC.Generics
import qualified Data.ByteString.Base64 as Base64
import qualified Data.ByteString.Lazy as LB
import           Data.Functor.Compose
import           Data.Text (Text)
import           Data.Text.Encoding (decodeLatin1, encodeUtf8)
import           GHC.Generics (Generic)

data DagNodeWithHeader
  = DagNodeWithHeader
  { node :: DagNode
  , header :: IPFSHeader
  } deriving (Show, Generic)

instance ToJSON DagNodeWithHeader where
    toEncoding = genericToEncoding defaultOptions
instance FromJSON DagNodeWithHeader

data DagNode
  = DagNode
  { node_data :: ByteString
  , links :: [IPFSHeader]
  } deriving (Show)

instance FromJSON DagNode where
    parseJSON = withObject "dag node" $ \o -> do
              d <- o .: "data"
              ls <- o .: "links"
              case Base64.decode (encodeUtf8 d) of
                Left err -> fail err
                Right x  -> pure $ DagNode x ls

instance ToJSON DagNode where
    toJSON (DagNode x ls)
      = object [ "data" .= decodeLatin1 (Base64.encode x)
               , "links" .= ls
               ]

newtype IPFSHash = IPFSHash { unIPFSHash :: Text } deriving (Show)

instance ToJSON IPFSHash where
  toJSON = String . unIPFSHash

instance FromJSON IPFSHash where
  parseJSON =
    withText "IPFSHash" (pure . IPFSHash)

instance ToHttpApiData IPFSHash where
  toUrlPiece = unIPFSHash

data IPFSHeader
  = IPFSHeader
  { name :: String
  , hash :: IPFSHash
  , size :: Int -- actually uint/u64
  } deriving (Generic, Show)

instance ToJSON IPFSHeader where
    toEncoding = genericToEncoding defaultOptions
instance FromJSON IPFSHeader

data GetResp
  = GetResp
  { requested_node :: DagNode
  , extra_node_count :: Int -- actually uint
  , extra_nodes :: [DagNodeWithHeader]
  } deriving (Generic, Show)

instance ToJSON GetResp where
    toEncoding = genericToEncoding defaultOptions
instance FromJSON GetResp


type MyApi = "get" :> Capture "hash" IPFSHash :> Get '[JSON] GetResp
        :<|> "object" :> "put" :> ReqBody '[JSON] DagNode :> Post '[JSON] IPFSHash

myApi :: Proxy MyApi
myApi = Proxy

-- 'client' allows you to produce operations to query an API from a client.
get :: IPFSHash  -> ClientM GetResp
put :: DagNode -> ClientM IPFSHash
(get :<|> put) = client myApi


runTest :: ClientM x -> IO x
runTest x = do
  manager' <- newManager defaultManagerSettings
  res <- runClientM x (mkClientEnv manager' (BaseUrl Http "localhost" 8088 ""))
  either (fail . show) pure res

